// 方法调用密集：覆盖 T6.8 的方法分派缓存。
//
// 每次 `xs.push(i)` 过去都要新建一个捕获接收者的闭包；缓存后同一个接收者上的
// 重复访问只需一次 `Value` 克隆。三种容器各取一个热方法，避免只测单一路径。
//
// 结果：xs.len = 60000，m["hits"] = 60000，s.len = 60
mut xs = []
mut m = { hits: 0 }
mut s = ""
for i in 60000 {
    xs.push(i)
    m.hits += 1
    if i < 60 { s += "x" }
    xs.contains(0)
}
xs.len
