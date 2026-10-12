pub mod builtins;
pub mod value;
pub mod vm;

pub use value::{Closure, NativeCallback, Value};
pub use vm::{Vm, VmError};

#[cfg(test)]
mod tests {
    use super::*;
    use ada_bytecode::Compiler;
    use ada_parser::Parser;

    fn eval(source: &str) -> Result<Value, String> {
        let mut parser = Parser::from_source(source).map_err(|e| e.to_string())?;
        let program = parser.parse_program().map_err(|e| e.to_string())?;
        let proto = Compiler::compile_program(&program).map_err(|e| e.to_string())?;
        let mut vm = Vm::new();
        vm.execute(proto).map_err(|e| e.to_string())
    }

    #[test]
    fn test_eval_arithmetic() {
        let res = eval("let a = 10\nlet b = 25\na * 2 + b").unwrap();
        assert_eq!(res, Value::Int(45));
    }

    #[test]
    fn test_eval_function_call() {
        let code = r#"
            fn add(x, y) = x + y
            add(100, 25)
        "#;
        let res = eval(code).unwrap();
        assert_eq!(res, Value::Int(125));
    }

    #[test]
    fn test_eval_recursion() {
        let code = r#"
            fn fib(n) {
                if n <= 1 { n } else { fib(n - 1) + fib(n - 2) }
            }
            fib(7)
        "#;
        let res = eval(code).unwrap();
        assert_eq!(res, Value::Int(13));
    }

    #[test]
    fn test_eval_pipe_and_sum() {
        let code = r#"
            let arr = [10, 20, 30]
            arr |> sum()
        "#;
        let res = eval(code).unwrap();
        assert_eq!(res, Value::Int(60));
    }

    #[test]
    fn test_eval_navigation_and_null_coalescing() {
        let code = r#"
            let user = {
                name: "Ada",
                score: 95
            }
            let valid = user?.score ?? 0
            let missing = user?.unknown?.field ?? 42
            valid + missing
        "#;
        let res = eval(code).unwrap();
        assert_eq!(res, Value::Int(137));
    }

    #[test]
    fn test_fuel_limit_stops_infinite_loop() {
        let code = r#"
            mut i = 0
            while true {
                i += 1
            }
        "#;
        let mut parser = Parser::from_source(code).unwrap();
        let program = parser.parse_program().unwrap();
        let proto = Compiler::compile_program(&program).unwrap();

        let mut vm = Vm::new();
        vm.set_fuel(Some(100)); // Only allow 100 instructions
        let err = vm.execute(proto).unwrap_err();
        assert!(matches!(err, VmError::OutOfFuel));
    }

    #[test]
    fn test_async_await_execution() {
        let code = r#"
            async fn calc(x, y) = x * 2 + y

            let task1 = spawn calc(10, 5)
            let task2 = spawn 50
            let r1 = task1.await
            let r2 = await task2
            r1 + r2
        "#;
        let res = eval(code).unwrap();
        assert_eq!(res, Value::Int(75));
    }

    #[test]
    fn test_spawn_and_await_all() {
        let code = r#"
            let t1 = spawn 10
            let t2 = spawn 20
            let t3 = spawn 30
            let all = [t1, t2, t3].await_all()
            all |> sum()
        "#;
        let res = eval(code).unwrap();
        assert_eq!(res, Value::Int(60));
    }

    #[test]
    fn test_iterate_array_and_range() {
        let code = r#"
            mut sum_val = 0
            for x in [10, 20, 30] {
                sum_val += x
            }
            for i in range(1, 4) {
                sum_val += i
            }
            sum_val
        "#;
        let res = eval(code).unwrap();
        assert_eq!(res, Value::Int(66)); // 10+20+30 + 1+2+3 = 66
    }

    #[test]
    fn test_iterate_string() {
        let code = r#"
            mut joined = ""
            for ch in "ada" {
                joined = joined + ch + "-"
            }
            joined
        "#;
        let res = eval(code).unwrap();
        assert_eq!(res, Value::string("a-d-a-"));
    }

    #[test]
    fn test_iterate_map_destructuring() {
        let code = r#"
            let data = {
                apple: 10,
                banana: 20
            }
            mut total = 0
            for [fruit, price] in data {
                total += price
            }
            total
        "#;
        let res = eval(code).unwrap();
        assert_eq!(res, Value::Int(30));
    }

    #[test]
    fn test_iterate_number_as_range() {
        let code = r#"
            mut count = 0
            for i in 5 {
                count += i
            }
            count
        "#;
        let res = eval(code).unwrap();
        assert_eq!(res, Value::Int(10)); // 0+1+2+3+4 = 10
    }

    #[test]
    fn test_builtins_core() {
        let code = r#"
            assert_eq(type_of(100), "int")
            assert_eq(type_of("hello"), "string")
            assert_eq(to_int("42"), 42)
            assert_eq(to_str(123), "123")
            assert_eq(min(5, 3), 3)
            assert_eq(max(5, 3), 5)
            assert_eq(clamp(15, 0, 10), 10)
            assert_eq(abs(-42), 42)
            true
        "#;
        let res = eval(code).unwrap();
        assert_eq!(res, Value::Bool(true));
    }

    #[test]
    fn test_builtins_json() {
        let code = r#"
            let raw = "{\"id\": 101, \"name\": \"Ada\"}"
            let parsed = json.parse(raw)
            assert_eq(parsed.id, 101)
            assert_eq(parsed.name, "Ada")
            assert_eq(json.valid(raw), true)
            assert_eq(json.valid("invalid json"), false)
            parsed.name
        "#;
        let res = eval(code).unwrap();
        assert_eq!(res, Value::string("Ada"));
    }

    #[test]
    fn test_builtins_math() {
        let code = r#"
            let s = math.sqrt(16.0)
            let p = math.pow(2.0, 3.0)
            assert_eq(s, 4.0)
            assert_eq(p, 8.0)
            math.PI > 3.14
        "#;
        let res = eval(code).unwrap();
        assert_eq!(res, Value::Bool(true));
    }

    #[test]
    fn test_builtins_time() {
        let code = r#"
            let start = time.now_ms()
            let secs = time.now_secs()
            let cost = time.elapsed_ms(start)
            assert(start > 0)
            assert(secs > 0)
            assert(cost >= 0)
            true
        "#;
        let res = eval(code).unwrap();
        assert_eq!(res, Value::Bool(true));
    }

    #[test]
    fn test_builtins_sys_and_env() {
        let code = r#"
            assert_eq(sys.version, "0.1.0")
            let fallback = env.get("NON_EXISTENT_VAR_KEY_XYZ", "default_val")
            assert_eq(fallback, "default_val")
            true
        "#;
        let res = eval(code).unwrap();
        assert_eq!(res, Value::Bool(true));
    }

    #[test]
    fn test_prototype_string_methods() {
        let code = r#"
            let s = "  Hello, Ada!  "
            assert_eq(s.trim(), "Hello, Ada!")
            assert_eq(s.trim().to_upper(), "HELLO, ADA!")
            assert_eq(s.contains("Ada"), true)
            assert_eq(s.starts_with("  Hello"), true)
            assert_eq(s.ends_with("!  "), true)
            assert_eq("abcdef".slice(1, 4), "bcd")
            assert_eq("foo bar".replace("bar", "baz"), "foo baz")
            true
        "#;
        let res = eval(code).unwrap();
        assert_eq!(res, Value::Bool(true));
    }

    #[test]
    fn test_prototype_array_methods() {
        let code = r#"
            mut arr = [10, 20]
            arr.push(30)
            assert_eq(arr.len, 3)
            let last = arr.pop()
            assert_eq(last, 30)
            assert_eq(arr.contains(20), true)
            assert_eq(arr.contains(99), false)
            assert_eq(arr.sum(), 30)
            true
        "#;
        let res = eval(code).unwrap();
        assert_eq!(res, Value::Bool(true));
    }

    #[test]
    fn test_prototype_map_methods() {
        let code = r#"
            mut m = { a: 1, b: 2 }
            assert_eq(m.contains_key("a"), true)
            assert_eq(m.contains_key("c"), false)
            let rem = m.remove("a")
            assert_eq(rem, 1)
            assert_eq(m.len, 1)
            m.merge({ c: 3 })
            assert_eq(m.c, 3)
            assert_eq(m.len, 2)
            true
        "#;
        let res = eval(code).unwrap();
        assert_eq!(res, Value::Bool(true));
    }
}
