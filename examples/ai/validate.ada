// AI 示例 3/3：记录校验（match + guard + 闭包 + 布尔取反）
//
// 运行：ada examples/ai/validate.ada
// 期望输出见同目录 validate.expected.txt

fn validate(record) {
    match record {
        r if r.age < 0 => { ok: false, reason: "negative age" },
        r if r.name == "" => { ok: false, reason: "empty name" },
        _ => { ok: true, reason: "" }
    }
}

let records = [
    { name: "ada", age: 36 },
    { name: "", age: 20 },
    { name: "bob", age: -1 }
]

let results = records |> map(r => validate(r)) |> collect()
let invalid = results |> filter(r => !r.ok) |> count()
let reasons = results
    |> filter(r => !r.ok)
    |> map(r => r.reason)
    |> collect()
    |> join("; ")

println($"checked={results.len} invalid={invalid}")
println($"reasons: {reasons}")

// 单条记录的即时校验：把闭包当值传递
let is_ok = validate
println($"direct: {is_ok({ name: "ada", age: 1 }).ok}")
