// fib(24)：递归调用与捕获表读取（调用开销主导）
fn fib(n) {
    if n <= 1 { n } else { fib(n - 1) + fib(n - 2) }
}
fib(24)
