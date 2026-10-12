// ============================================================================
// Ada-D 规范示例 showcase（golden 脚本）
//
// 用途：把 SPEC_LANGUAGE §5–§8、SPEC_STDLIB §2–§6、SPEC_ASYNC §2 中的示例
// 汇总成一段**确定性**输出，由 crates/ada_cli/tests/golden_showcase.rs 比对
// examples/showcase.expected.txt，防止文档与实现漂移（风险 R1）。
//
// 确定性要求：不使用 time/random/sys，不打印多键 map（HashMap 顺序不保证）。
// ============================================================================

// ---------------------------------------------------------------- §5.1 变量
println("-- variables --")
let immutable_val = 10
mut counter = 0
counter += 1
println($"immutable={immutable_val} counter={counter}")

let shadowed = 1
{
    let shadowed = 2
    println($"inner shadow={shadowed}")
}
println($"outer shadow={shadowed}")

// -------------------------------------------------------------- §5.2 函数
println("-- functions --")
fn add(x, y) = x + y
fn bonus(score, base) {
    let multiplier = if score > 90 { 1.5 } else { 1.0 }
    base * multiplier
}
fn fib(n) {
    if n <= 1 { n } else { fib(n - 1) + fib(n - 2) }
}
fn early(n) {
    if n < 0 { return "negative" }
    "non-negative"
}
println($"add={add(2, 3)} bonus={bonus(95, 100)} fib(10)={fib(10)} early(-1)={early(-1)}")

// ------------------------------------------------------------ §5.3 控制流
println("-- control flow --")
mut total = 0
for i in range(1, 5) { total += i }
println($"range sum={total}")

mut chars = ""
for ch in "Ada" { chars += ch + "." }
println($"chars={chars}")

mut countdown = 5
mut steps = 0
while countdown > 0 {
    countdown -= 1
    if countdown == 3 { continue }
    if countdown == 1 { break }
    steps += 1
}
println($"while steps={steps}")

mut breaks = 0
for i in 0 { breaks += 1 }
for i in 3 { breaks += i }
println($"int range sum={breaks}")

// ------------------------------------------------- §5.5 索引、长度与切片
println("-- indexing --")
let text = "Ada 语言"
println($"len={text.len} bytes={text.len_bytes} first={text[0]} char4={text[4]} oob={text[99]}")
println($"slice={text.slice(0, 3)} reversed_join={["a", "b", "c"].rev().join("-")}")

// ------------------------------------------------------------- §6 闭包
println("-- closures --")
fn make_adder(n) { x => x + n }
let add5 = make_adder(5)
mut hits = 0
let bump = () => { hits += 1
    hits }
bump()
bump()
println($"add5(1)={add5(1)} hits={hits}")

let grabs = []
for i in range(0, 3) { grabs.push((() => i)) }
println($"loop captures={[grabs[0](), grabs[1](), grabs[2]()]}")

// ------------------------------------------------------------- §7 模式匹配
println("-- match --")
fn grade(score) {
    match score {
        >= 90 => "A",
        >= 80 => "B",
        >= 60 => "C",
        _ => "F"
    }
}
fn kind(value) {
    match value {
        0 => "zero",
        n if n < 0 => "negative",
        _ => "positive"
    }
}
println($"grade(95)={grade(95)} grade(85)={grade(85)} grade(61)={grade(61)} grade(10)={grade(10)}")
println($"kind(0)={kind(0)} kind(-5)={kind(-5)} kind(7)={kind(7)}")

// -------------------------------------------------------- §8 安全导航
println("-- navigation --")
let agent = {
    model: "ada-mini",
    meta: { timeout: 3000 },
    tags: ["web", "calc"]
}
println($"timeout={agent?.meta?.timeout ?? 5000} missing={agent?.missing?.deep ?? "fallback"}")
println($"tag0={agent.tags[0]} len={agent.tags.len}")

// ------------------------------------------------------ §9 相等与真值
println("-- equality --")
let xs = [1, 2]
println($"scalar={1 == 1.0} str={"a" == "a"} same_ref={xs == xs} diff_ref={[1, 2] == [1, 2]}")
println($"truthy={to_int(to_bool("x"))} falsy={to_int(to_bool(""))} nil_falsy={to_int(to_bool(nil))}")

// -------------------------------------------------- SPEC_STDLIB §2 全局函数
println("-- stdlib globals --")
println($"type_of={type_of(1)}/{type_of("s")}/{type_of([1])}/map:{type_of({ a: 1 })}/{type_of(add)}")
println($"conv={to_int("42")}/{to_float(3)}/{to_str(7)}")
println($"math={abs(-4)}/{min(3, 1, 2)}/{max([3, 1, 2])}/{clamp(15, 0, 10)}/{round(3.6)}/{floor(3.9)}/{ceil(3.1)}")
println($"assert={assert(1 < 2)}/{assert_eq(add(1, 2), 3)}")

// ---------------------------------------------- SPEC_STDLIB §3 原生模块
println("-- stdlib modules --")
let raw = "{\"id\": 101, \"name\": \"Ada\", \"tags\": [\"rust\", \"ai\"], \"active\": true}"
let parsed = json.parse(raw)
println($"json={parsed.name}/{parsed.tags[1]}/{parsed.active}/{parsed.id} valid={json.valid(raw)}/{json.valid("{oops}")}")
println($"stringify={json.stringify({ ok: true })}")
println($"mathmod={math.sqrt(256.0)}/{math.pow(2.0, 8.0)}/{to_int(math.PI * 1000.0)}")

// -------------------------------------------- SPEC_STDLIB §4 原型方法
println("-- prototypes --")
let padded = "  Ada Script  "
println($"string={padded.trim()}/{padded.trim().to_upper()}/{padded.trim().to_lower()}")
println($"split={("a,b,c").split(",").join("+")} replace={"foo bar".replace("bar", "baz")}")
println($"probe={"abcdef".contains("cd")}/{("abcdef").starts_with("ab")}/{("abcdef").ends_with("ef")}/{"abcdef".slice(1, 4)}")

mut arr = [3, 1, 2]
arr.push(4)
arr.insert(0, 0)
let removed = arr.remove(0)
arr.sort()
println($"array={arr} removed={removed} contains={arr.contains(3)}/{arr.contains(9)}")
println($"array_rev={arr.rev()} popped={arr.pop()} now={arr}")

mut cfg = { retries: 3, verbose: true }
let before = cfg.len
cfg.merge({ verbose: false })
let dropped = cfg.remove("retries")
println($"map=len:{before}->{cfg.len} dropped:{dropped} has_key:{cfg.contains_key("verbose")}")

// --------------------------------- SPEC_STDLIB §5–§6 迭代与适配器
println("-- iteration --")
for [idx, item] in ["x", "y"].enumerate() { print($"{idx}:{item} ") }
println("")
mut lazy = 0
for n in range(1, 6) { lazy += n }
println($"range={lazy}")

println($"pipeline={range(1, 1000) |> filter(x => x % 2 != 0) |> map(x => x * x) |> take(5) |> sum()}")
println($"method={[1, 2, 3, 4].map(x => x * 10).filter(x => x > 20).collect()}")
println($"fold={[4, 5, 6].count()}/{["a", "b"].join("|")}/{[9, 8, 7].find(x => x < 9)}")
println($"pred={[1, 2, 3].any(x => x > 2)}/{[1, 2, 3].all(x => x > 0)}/{[].any(x => true)}/{[].all(x => false)}")
let explicit = [7, 8].iter()
println($"explicit={explicit.next()}/{explicit.next()}/{explicit.next()}")
println($"chars_with_idx={"ab".chars_with_idx()}")

// ------------------------------------------------ SPEC_ASYNC §2 异步语法
println("-- async syntax --")
async fn fetch_service(name, latency) {
    latency * 2
}
let task1 = spawn fetch_service("auth", 5)
let task2 = spawn 50
println($"async={task1.await}/{await task2}/{[spawn 1, spawn 2].await_all()}")

// M4：协作式调度 —— yield_now 的交错顺序是确定的（不含真实时间）
mut trace = ""
fn tracer(tag) {
    for i in 2 { trace += tag
        yield_now() }
    tag
}
let trace_a = spawn tracer("a")
let trace_b = spawn tracer("b")
let traced = [trace_a, trace_b].await_all()
println($"async_interleave={trace} tasks={traced} state={trace_a.state}")

// ------------------------------------------------ M3 语义收紧（T3.1 / T3.5 / T3.6）
println("-- semantics --")
let frozen = 10
mut flexible = 10
flexible += 1
println($"immutable={frozen} mutable={flexible}")

let numbers = [1, 2, 3]
println($"neg_index={numbers[-1]}/{"abc"[-1]}/{"abc"[-9]} tail_write={numbers[0]}")

mut writable = [1, 2, 3]
writable[-1] = 9
println($"tail_write={writable} mixed_mod={5.0 % 2} int_mod={5 % 2}")

println($"compare={to_int(1 < 2.5)} nan_cmp={to_int((0.0 / 0.0) < 1.0)} float_str={to_str(1.0)}/{to_str(1.5)}")

// ------------------------------------------------ 断言收尾（自校验）
assert_eq(range(1, 1000) |> filter(x => x % 2 != 0) |> map(x => x * x) |> take(5) |> sum(), 165)
assert_eq(fib(10), 55)
println("-- showcase complete --")
