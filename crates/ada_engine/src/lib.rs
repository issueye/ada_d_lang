use ada_bytecode::Compiler;
use ada_parser::Parser;
use ada_vm::{Value, Vm, VmError};
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
    pub fn new() -> Self {
        Self { vm: Vm::new() }
    }

    pub fn set_fuel(&mut self, fuel: Option<u64>) {
        self.vm.set_fuel(fuel);
    }

    pub fn get_fuel(&self) -> Option<u64> {
        self.vm.get_fuel()
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
