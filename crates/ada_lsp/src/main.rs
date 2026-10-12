//! `ada-lsp` 可执行入口：在 stdio 上跑最小 LSP 服务器（开发计划 T7.2）。
//!
//! 用法（编辑器按 LSP 客户端惯例调用）：
//!
//! ```text
//! ada-lsp --stdio
//! ```
//!
//! 诊断内容与 `ada check` 完全一致（同一份 `Engine::check` 代码路径），
//! 因此编辑器里的波浪线与终端报错不会打架。

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("ada-lsp {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!(
            "ada-lsp {} —— Ada-D 最小 LSP 服务器（静态诊断）",
            env!("CARGO_PKG_VERSION")
        );
        println!();
        println!("用法: ada-lsp [--stdio]");
        println!("  通过 stdin/stdout 收发 JSON-RPC（Content-Length 分帧）。");
        println!("  --stdio 仅为兼容 LSP 客户端惯例，可省略。");
        return;
    }

    match ada_lsp::run_stdio() {
        Ok(true) => {}
        // 没收到 shutdown 就退出：按 LSP 规范以非零码结束（但仍不 panic）
        Ok(false) => std::process::exit(1),
        Err(err) => {
            eprintln!("ada-lsp: {err}");
            std::process::exit(1);
        }
    }
}
