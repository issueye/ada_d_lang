// AI 示例 2/3：并发等待与超时（spawn / await_any / await_all / timeout）
//
// 运行：ada examples/ai/async.ada
// 期望输出见同目录 async.expected.txt

fn call_service(name, latency_ms) {
    sleep_ms(latency_ms)
    { name: name, ms: latency_ms }
}

// 1) 最先就绪者：search(10ms) 比 billing(40ms) 先完成
let slow = spawn call_service("billing", 40)
let fast = spawn call_service("search", 10)
println($"first ready: {[slow, fast].await_any().name}")

// 2) 全部汇聚：按实参顺序返回（不是完成顺序）
let all = [
    spawn call_service("auth", 20),
    spawn call_service("billing", 40),
    spawn call_service("search", 10)
].await_all()
println($"all: {all |> map(r => r.name) |> join(",")}")

// 3) 超时：挂起的任务被取消，timeout 返回 nil
fn hung() {
    sleep_ms(5000)
    "late"
}
let h = spawn hung()
let guarded = h.timeout(20)
println($"timeout: {guarded} state: {h.state}")
