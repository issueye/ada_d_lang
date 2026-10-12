// JSON 往返：原生模块 + 容器构造
let raw = "{\"id\": 101, \"tags\": [\"rust\", \"ai\", \"vm\"], \"meta\": {\"retries\": 3, \"verbose\": true}, \"items\": [1, 2, 3, 4, 5, 6, 7, 8]}"
mut total = 0
for i in 3000 {
    let parsed = json.parse(raw)
    total += parsed.meta.retries + parsed.tags.len
}
total
