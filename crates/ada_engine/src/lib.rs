use ada_bytecode::Compiler;
use ada_parser::Parser;
// 公共 re-export：宿主只需依赖 ada_engine 即可命名 Value 与各类错误类型（T0.7）
pub use ada_bytecode::CompileError;
pub use ada_parser::ParseError;
pub use ada_vm::{
    AsyncKind, Closure, NativeCallback, ProfileKind, Sandbox, TaskCell, TaskState, Value, Vm,
    VmError,
};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Parse error: {0}")]
    Parse(#[from] ada_parser::ParseError),

    #[error("Compile error: {0}")]
    Compile(#[from] ada_bytecode::CompileError),

    #[error("VM error: {0}")]
    Vm(#[from] VmError),

    #[error("Type conversion error: {0}")]
    Conversion(String),

    #[error("Module '{0}' is already registered")]
    ModuleConflict(String),
}

impl Error {
    /// 错误位置（T1.8）：词法/语法错误自带 `line:col`，编译错误自带 `Span`；
    /// 运行时错误的位置由 [`Engine::eval_diagnostic`] 通过 [`EvalFailure`] 附带
    /// （`Error` 本身不持有 VM 状态）。
    pub fn position(&self) -> Option<Position> {
        match self {
            Error::Parse(parse_err) => {
                let (line, col) = parse_err.position();
                Some(Position { line, col })
            }
            Error::Compile(compile_err) => compile_err.span().map(Position::from_span),
            Error::Vm(_) | Error::Conversion(_) | Error::ModuleConflict(_) => None,
        }
    }
}

/// 源码位置（行/列，均从 1 开始）——CLI 的 `file:line:col` 与将来 LSP 诊断共用（T1.8/T7.2）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    pub col: usize,
}

impl Position {
    pub fn from_span(span: ada_ast::Span) -> Self {
        Self {
            line: span.line,
            col: span.col,
        }
    }
}

/// 求值失败：原因 + 位置（T1.8）。
///
/// 与 `Error` 的关系：`Error` 是错误**种类**（可由宿主自行匹配），
/// `EvalFailure` 是"某次求值**在哪里**失败了"这一信息，因此只在 `Engine` 边界产生。
#[derive(Debug)]
pub struct EvalFailure {
    /// 底层错误（保留类型信息，宿主可继续匹配）
    pub error: Error,
    /// 失败位置；无位置可归因时为 `None`
    pub position: Option<Position>,
}

impl EvalFailure {
    /// 便于 CLI/日志直接打印的人话消息。
    pub fn message(&self) -> String {
        self.error.to_string()
    }
}

impl std::fmt::Display for EvalFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.position {
            Some(pos) => write!(f, "{}:{}: {}", pos.line, pos.col, self.error),
            None => write!(f, "{}", self.error),
        }
    }
}

impl std::error::Error for EvalFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.error)
    }
}

/// 宿主自定义原生模块（M5 / T5.5）。
///
/// 实现后通过 [`Engine::register_module`] 注册；脚本侧表现为一个 `map` 值，
/// 因此 `mymod.fn(args)` 与 `mymod.fn` 取函数引用都可用。
///
/// 模块是**宿主显式授予的能力**，因此不受沙箱 profile 限制（沙箱限制的是
/// AdA-D 自带模块）；宿主可自行决定暴露什么。
pub trait NativeModule: Send + Sync {
    /// 模块名（脚本里的全局名）
    fn name(&self) -> &str;

    /// 模块暴露的函数：`(函数名, 实现)`
    fn functions(&self) -> Vec<(String, NativeCallback)>;
}

pub trait FromValue: Sized {
    fn from_value(val: Value) -> Result<Self, Error>;
}

impl FromValue for Value {
    fn from_value(val: Value) -> Result<Self, Error> {
        Ok(val)
    }
}

impl FromValue for i64 {
    fn from_value(val: Value) -> Result<Self, Error> {
        val.as_int()
            .ok_or_else(|| Error::Conversion(format!("Expected int, got {val:?}")))
    }
}

impl FromValue for f64 {
    fn from_value(val: Value) -> Result<Self, Error> {
        val.as_float()
            .ok_or_else(|| Error::Conversion(format!("Expected float, got {val:?}")))
    }
}

impl FromValue for bool {
    fn from_value(val: Value) -> Result<Self, Error> {
        match val {
            Value::Bool(b) => Ok(b),
            _ => Err(Error::Conversion(format!("Expected bool, got {val:?}"))),
        }
    }
}

impl FromValue for String {
    fn from_value(val: Value) -> Result<Self, Error> {
        match val {
            Value::String(s) => Ok((*s).clone()),
            _ => Ok(val.to_string()),
        }
    }
}

impl FromValue for serde_json::Value {
    fn from_value(val: Value) -> Result<Self, Error> {
        Ok(val.to_json())
    }
}

pub struct Engine {
    vm: Vm,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    /// 默认引擎：**strict 沙箱**（M5 / T5.1）——只有纯计算 + `json`/`math`，
    /// 隐藏 `time`/`env`/`sys`/`sleep_ms`，并施加内存/墙钟/任务数上限。
    ///
    /// 需要宿主能力时显式选择 [`Engine::with_sandbox`]：
    /// `Engine::with_sandbox(Sandbox::standard())` 或按需构造
    /// `Sandbox::strict().with_env_keys(["HOME"]).with_wall_clock_limit(Some(10_000))`。
    pub fn new() -> Self {
        Self::with_sandbox(Sandbox::strict())
    }

    /// 用给定沙箱配置构建引擎。
    pub fn with_sandbox(sandbox: Sandbox) -> Self {
        Self {
            vm: Vm::with_sandbox(sandbox),
        }
    }

    /// 不做能力/资源限制（等效 M5 之前的行为；仅用于受信场景与测试）。
    pub fn permissive() -> Self {
        Self::with_sandbox(Sandbox::permissive())
    }

    /// 当前沙箱配置。
    pub fn sandbox(&self) -> &Sandbox {
        self.vm.sandbox()
    }

    /// 重新配置沙箱（会重建全局表，脚本写入的全局会丢失）。
    pub fn set_sandbox(&mut self, sandbox: Sandbox) {
        self.vm.set_sandbox(sandbox);
    }

    /// 本次求值已计费的近似内存字节数（M5 / T5.3）。
    pub fn memory_used(&self) -> usize {
        self.vm.memory_used()
    }

    /// 注册一个宿主原生模块（M5 / T5.5）。
    pub fn register_module<M: NativeModule + 'static>(&mut self, module: M) -> Result<(), Error> {
        let name = module.name().to_string();
        if self.vm.get_global(&name).is_some() {
            return Err(Error::ModuleConflict(name));
        }
        let mut map = HashMap::new();
        for (fname, callback) in module.functions() {
            let qualified = format!("{name}.{fname}");
            map.insert(fname, Value::native_fn_shared(qualified, callback));
        }
        self.vm.set_global(name, Value::map(map));
        Ok(())
    }

    pub fn set_fuel(&mut self, fuel: Option<u64>) {
        self.vm.set_fuel(fuel);
    }

    pub fn get_fuel(&self) -> Option<u64> {
        self.vm.get_fuel()
    }

    /// 当前这次求值剩余的可执行指令数（T2.6）。
    pub fn remaining_fuel(&self) -> Option<u64> {
        self.vm.remaining_fuel()
    }

    /// 把剩余燃料重置为配置的预算（T2.6）。
    /// 每次 `eval*` 也会自动重新装填，因此一次 `OutOfFuel` 不会让 `Engine` 永久失效。
    pub fn reset_fuel(&mut self) {
        self.vm.reset_fuel()
    }

    pub fn register_fn(
        &mut self,
        name: impl Into<String>,
        cb: impl Fn(&[Value]) -> Result<Value, String> + Send + Sync + 'static,
    ) {
        let name_str = name.into();
        self.vm
            .set_global(name_str.clone(), Value::native_fn(name_str, cb));
    }

    pub fn set_global(&mut self, name: impl Into<String>, val: Value) {
        self.vm.set_global(name, val);
    }

    pub fn get_global(&self, name: &str) -> Option<&Value> {
        self.vm.get_global(name)
    }

    pub fn eval_value(&mut self, source: &str) -> Result<Value, Error> {
        self.eval_diagnostic(source)
            .map_err(|failure| failure.error)
    }

    /// 求值并在失败时附带**位置**（T1.8）。
    ///
    /// 这是 CLI 与将来 LSP 使用的入口：解析/编译错误的位置来自
    /// `ParseError::position` / `CompileError::span`，运行时错误来自 VM 记录的
    /// "失败指令"位置。
    ///
    /// ```
    /// # use ada_engine::Engine;
    /// let mut engine = Engine::new();
    /// // 第 2 行触发运行时错误（除零），位置应指向该行的 `1 / 0`
    /// let failure = engine.eval_diagnostic("let a = 1\nlet b = 1 / 0").unwrap_err();
    /// assert_eq!(failure.position.map(|p| p.line), Some(2));
    /// ```
    pub fn eval_diagnostic(&mut self, source: &str) -> Result<Value, EvalFailure> {
        let parsed = Parser::from_source(source).and_then(|mut parser| parser.parse_program());
        let program = match parsed {
            Ok(program) => program,
            Err(err) => {
                let (line, col) = err.position();
                return Err(EvalFailure {
                    error: Error::Parse(err),
                    position: Some(Position { line, col }),
                });
            }
        };
        let proto = match Compiler::compile_program(&program) {
            Ok(proto) => proto,
            Err(err) => {
                return Err(EvalFailure {
                    position: err.span().map(Position::from_span),
                    error: Error::Compile(err),
                });
            }
        };
        match self.vm.execute(proto) {
            Ok(val) => Ok(val),
            Err(err) => {
                let position = self.vm.last_error_span().map(Position::from_span);
                Err(EvalFailure {
                    error: Error::Vm(err),
                    position,
                })
            }
        }
    }

    pub fn eval<T: FromValue>(&mut self, source: &str) -> Result<T, Error> {
        let val = self.eval_value(source)?;
        T::from_value(val)
    }

    /// **静态检查**：只做词法 / 语法 / 编译检查，**不执行**脚本（T7.2）。
    ///
    /// 编辑器（LSP）与 CI 需要"判断这段代码能不能编译"而不想承担执行代价与副作用：
    /// `eval_diagnostic` 会真的跑脚本（可能 `sleep`、读写宿主、触发沙箱限额），
    /// 因此这里提供一条纯静态路径。返回的错误与求值路径同源（`ParseError` /
    /// `CompileError` + `Position`），保证编辑器里的波浪线与 `ada run` 的报错一致。
    ///
    /// 注意：运行时错误（如未定义全局变量、类型错误）需要执行才知道，不在本方法范围内。
    ///
    /// ```
    /// # use ada_engine::Engine;
    /// assert!(Engine::check("let a = 1\na + 1").is_ok());
    /// let failure = Engine::check("let a = 1\na = 2").unwrap_err();
    /// assert_eq!(failure.position.map(|p| p.line), Some(2));
    /// ```
    pub fn check(source: &str) -> Result<(), EvalFailure> {
        let parsed = Parser::from_source(source).and_then(|mut parser| parser.parse_program());
        let program = match parsed {
            Ok(program) => program,
            Err(err) => {
                let (line, col) = err.position();
                return Err(EvalFailure {
                    error: Error::Parse(err),
                    position: Some(Position { line, col }),
                });
            }
        };
        match Compiler::compile_program(&program) {
            Ok(_) => Ok(()),
            Err(err) => Err(EvalFailure {
                position: err.span().map(Position::from_span),
                error: Error::Compile(err),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_eval_primitives() {
        let mut engine = Engine::new();
        let res: i64 = engine.eval("let a = 15; a * 4").unwrap();
        assert_eq!(res, 60);

        let s: String = engine.eval(r#"$"Score: {100}""#).unwrap();
        assert_eq!(s, "Score: 100");
    }

    #[test]
    fn test_engine_register_fn() {
        let mut engine = Engine::new();
        engine.register_fn("multiply", |args| {
            if args.len() < 2 {
                return Err("Expected 2 arguments".into());
            }
            let a = args[0].as_float().unwrap_or(0.0);
            let b = args[1].as_float().unwrap_or(0.0);
            Ok(Value::Float(a * b))
        });

        let res: f64 = engine.eval("multiply(6.0, 7.0)").unwrap();
        assert_eq!(res, 42.0);
    }

    #[test]
    fn test_engine_fuel() {
        let mut engine = Engine::new();
        engine.set_fuel(Some(50));
        let err = engine.eval::<Value>("while true {}").unwrap_err();
        assert!(matches!(err, Error::Vm(VmError::OutOfFuel)));
    }
}
