// 1. 变量与单行函数
let base = 100
fn add(x, y) = x + y + base

// 2. 递归函数
fn fib(n) {
    if n <= 1 { n } else { fib(n - 1) + fib(n - 2) }
}

// 3. 对象与安全链式导航
let user = {
    name: "Ada",
    roles: ["admin", "developer"],
    config: {
        timeout: 3000
    }
}

let timeout = user?.config?.timeout ?? 5000
let unknown = user?.missing?.item ?? "fallback"

// 4. 字符串插值 (Token 极低)
println($"User {user.name} has {user.roles.len} roles, timeout: {timeout}")

// 5. 异步函数与并发任务 (Async / Await / Spawn)
async fn fetch_service(name, latency_ms) {
    sleep_ms(latency_ms)
    $"Service [{name}] ready"
}

// 并发启动任务 (spawn)
let task1 = spawn fetch_service("auth", 5)
let task2 = spawn fetch_service("database", 10)

// 支持后缀 .await 与前缀 await
let res1 = task1.await
let res2 = await task2
println($"Async result 1: {res1}")
println($"Async result 2: {res2}")

// 并发汇聚 (await_all) 与管道汇总
let task_scores = [spawn 100, spawn 200, spawn 300]
let scores = task_scores.await_all()
let total = scores |> sum()
println($"Concurrently aggregated scores total: {total}")

// 6. 统一迭代器规范演示 (Array, String, Map, Range, Destructuring)
mut letters = ""
for ch in "Ada-D" {
    letters = letters + ch + "."
}
println($"String chars iteration: {letters}")

mut cart_total = 0
let cart = {
    book: 50,
    pen: 10,
    coffee: 25
}
for [item, price] in cart {
    cart_total += price
}
println($"Map destructured iteration total: {cart_total}")

mut range_sum = 0
for i in range(1, 5) {
    range_sum += i
}
println($"Lazy range iteration sum (1..5): {range_sum}")

// 7. 原生标准库与内置全局方法 (Stdlib: json, math, time, sys, env & Prototypes)
let json_str = "{\"model\": \"claude-3-5\", \"tokens\": 256, \"stream\": true}"
let parsed = json.parse(json_str)
println($"JSON parsed model: {parsed.model}, tokens: {parsed.tokens}")

let root_val = math.sqrt(256.0)
let power = math.pow(2.0, 8.0)
println($"Math sqrt(256): {root_val}, 2^8: {power}, PI: {math.PI}")

let start_time = time.now_ms()
sleep_ms(5)
let cost_ms = time.elapsed_ms(start_time)
println($"Execution monitored by time module took: {cost_ms}ms")

println($"Ada-D Runtime Version: {sys.version}, OS: {sys.os}")

// 字符串与集合原型扩展方法
let raw_tag = "  ada_script  "
let clean_tag = raw_tag.trim().to_upper()
println($"Cleaned tag: {clean_tag}, contains ADA: {clean_tag.contains("ADA")}")

mut dynamic_arr = [10, 20]
dynamic_arr.push(30)
println($"Dynamic array len: {dynamic_arr.len}, sum: {dynamic_arr.sum()}")

// 8. 返回最后表达式计算值
fib(10)

