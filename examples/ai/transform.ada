import json from "std:json"
// AI 示例 1/3：把 JSON 报告规整成表格（json + 适配器管道 + map 字面量）
//
// 运行：ada examples/ai/transform.ada
// 期望输出见同目录 transform.expected.txt（由 golden 测试比对）

let raw = "{
  \"events\": [
    { \"name\": \"parse\",   \"severity\": \"info\",  \"score\": 3 },
    { \"name\": \"lex\",     \"severity\": \"debug\", \"score\": 7 },
    { \"name\": \"compile\", \"severity\": \"warn\",  \"score\": 5 },
    { \"name\": \"run\",     \"severity\": \"info\",  \"score\": 11 }
  ]
}"

let report = json.parse(raw)

// 管道：过滤 → 映射成只需要两个字段的记录 → 收集
let rows = report.events
    |> filter(e => e.severity != "debug")
    |> map(e => { name: e.name, doubled: e.score * 2 })
    |> collect()

println("name      doubled")
for row in rows {
    println($"{row.name}      {row.doubled}")
}

let total = rows |> map(r => r.doubled) |> sum()
let loudest = rows |> map(r => r.doubled) |> max()
println($"rows={rows.len} total={total} max={loudest}")
