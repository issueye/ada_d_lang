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
            map.insert(
                fname,
                Value::NativeFn {
                    name: qualified,
                    callback,
                },
            );
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
        let mut parser = Parser::from_source(source)?;
        let program = parser.parse_program()?;
        let proto = Compiler::compile_program(&program)?;
        let val = self.vm.execute(proto)?;
        Ok(val)
    }

    pub fn eval<T: FromValue>(&mut self, source: &str) -> Result<T, Error> {
        let val = self.eval_value(source)?;
        T::from_value(val)
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
