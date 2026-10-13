use ada_bytecode::Compiler;
use ada_engine::{Engine, Sandbox};
use ada_parser::Parser;
use clap::{Parser as ClapParser, Subcommand};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(ClapParser)]
#[command(
    name = "ada",
    author = "Ada-D Team",
    version = "0.1.0",
    about = "Ada-D Programming Language CLI"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Script file to run directly (e.g. `ada script.ada`)
    #[arg(index = 1)]
    file: Option<PathBuf>,

    /// Evaluate code directly from string (e.g. `ada -e "1 + 2"`)
    #[arg(short = 'e', long = "eval")]
    eval: Option<String>,

    /// Use the strict sandbox (no time/env/sys/sleep_ms; memory & wall-clock limits)
    #[arg(long = "strict", global = true)]
    strict: bool,

    /// Expose the `fs` module, restricted to this directory (T7.5)
    #[arg(long = "fs-root", global = true, value_name = "DIR")]
    fs_root: Option<PathBuf>,

    /// Allow file imports, restricted to this directory (T8.9)
    #[arg(long = "import-root", global = true, value_name = "DIR")]
    import_root: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a script file
    Run {
        /// Path to the .ada script file
        file: PathBuf,
    },
    /// Evaluate code string directly
    Eval {
        /// Code string to execute
        code: String,
    },
    /// Dump disassembled bytecode for a script file
    Dump {
        /// Path to the .ada script file
        file: PathBuf,
    },
    /// Check a script **without running it** (syntax + compile diagnostics only)
    Check {
        /// Path to the .ada script file
        file: PathBuf,
    },
    /// Launch interactive REPL session
    Repl,
}

fn main() {
    let cli = Cli::parse();
    let sandbox = if cli.strict {
        Sandbox::strict()
    } else {
        // CLI 是本地受信工具：默认 standard（time/sys/env 白名单/sleep 可用），
        // 但对宿主环境变量仍然只放行显式白名单。
        Sandbox::standard()
    };
    // T7.5：只有显式给出 `--fs-root` 才暴露 fs 模块，且限制在该目录内
    let sandbox = match &cli.fs_root {
        Some(root) => sandbox.with_fs_root(root),
        None => sandbox,
    };

    // T0.6：统一错误格式 `[file:]line:col: error: <消息>`（有位置时）或 `<file>: error: <消息>`。
    // 自行捕获错误而不依赖 `main -> Result` 的 Debug 输出。
    let source_name = source_name(&cli);

    let result = dispatch(&cli, &sandbox);
    if let Err(err) = result {
        eprintln!("{}", render_error(&source_name, err.as_ref()));
        std::process::exit(1);
    }
}

fn dispatch(cli: &Cli, sandbox: &Sandbox) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(code) = &cli.eval {
        return run_code(code, sandbox);
    }

    match &cli.command {
        Some(Commands::Run { file }) => run_file(file, sandbox, cli.import_root.as_deref()),
        Some(Commands::Eval { code }) => run_code(code, sandbox),
        Some(Commands::Dump { file }) => dump_bytecode(file),
        Some(Commands::Check { file }) => check_file(file),
        Some(Commands::Repl) => run_repl(sandbox),
        None => {
            if let Some(file) = &cli.file {
                run_file(file, sandbox, cli.import_root.as_deref())
            } else {
                run_repl(sandbox)
            }
        }
    }
}

/// 报错前缀用的"来源名"：子命令里的文件路径优先于位置参数（T0.6）。
///
/// 少了这一步，`ada check script.ada` 的报错会写成 `<eval>:2:1: …`，
/// 编辑器/CI 就丢掉了文件名。
fn source_name(cli: &Cli) -> String {
    let path = match &cli.command {
        Some(Commands::Run { file })
        | Some(Commands::Dump { file })
        | Some(Commands::Check { file }) => Some(file),
        _ => cli.file.as_ref(),
    };
    path.map(|p| p.display().to_string())
        .unwrap_or_else(|| "<eval>".to_string())
}

/// 把错误渲染成稳定的、面向用户与工具的单行格式（T0.6 / T1.8）。
///
/// - 有位置时 `file:line:col: error: <消息>`；无位置时 `file: error: <消息>`；
/// - 消息用 `Display`（人话）而非 `Debug`；
/// - 词法/语法错误保留 `expected` 细节。
fn render_error(source_name: &str, err: &(dyn std::error::Error + 'static)) -> String {
    // 求值入口（带位置，T1.8）
    if let Some(failure) = err.downcast_ref::<ada_engine::EvalFailure>() {
        let detail = render_detail(&failure.error);
        return match failure.position {
            Some(pos) => format!("{source_name}:{}:{}: error: {detail}", pos.line, pos.col),
            None => format!("{source_name}: error: {detail}"),
        };
    }
    // 其他路径（反汇编、文件读取等）：直接返回的引擎错误
    if let Some(engine_err) = err.downcast_ref::<ada_engine::Error>() {
        let detail = render_detail(engine_err);
        return match engine_err.position() {
            Some(pos) => format!("{source_name}:{}:{}: error: {detail}", pos.line, pos.col),
            None => format!("{source_name}: error: {detail}"),
        };
    }
    format!("{source_name}: error: {err}")
}

/// 错误种类 → 一行消息（不含位置，位置由调用方作为前缀给出）。
fn render_detail(err: &ada_engine::Error) -> String {
    match err {
        ada_engine::Error::Parse(parse_err) => parse_message(parse_err),
        ada_engine::Error::Compile(compile_err) => compile_err.to_string(),
        ada_engine::Error::Vm(vm_err) => vm_err.to_string(),
        ada_engine::Error::Conversion(msg) => msg.clone(),
        ada_engine::Error::ModuleConflict(name) => {
            format!("module '{name}' is already registered")
        }
        ada_engine::Error::Import { message, .. } => message.clone(),
    }
}

/// 语法错误的一行人话（位置由前缀给出，因此从消息里去掉 "at line …"）。
fn parse_message(err: &ada_parser::ParseError) -> String {
    match err {
        ada_parser::ParseError::UnexpectedToken {
            found, expected, ..
        } => format!("unexpected token {found}, expected {expected}"),
        ada_parser::ParseError::UnexpectedEof { expected, .. } => {
            format!("unexpected end of input, expected {expected}")
        }
        ada_parser::ParseError::InvalidAssignmentTarget { .. } => {
            "invalid assignment target".to_string()
        }
        ada_parser::ParseError::Lex(lex) => strip_position(&lex.to_string()),
    }
}

/// 去掉 `… at line X, col Y` 尾巴（位置已由前缀给出）。
fn strip_position(text: &str) -> String {
    match text.rfind(" at line ") {
        Some(idx) => text[..idx].to_string(),
        None => text.to_string(),
    }
}

fn run_file(
    path: &Path,
    sandbox: &Sandbox,
    import_root: Option<&Path>,
) -> Result<(), Box<dyn std::error::Error>> {
    // T8.7：走带文件路径的求值入口，使相对 import 可解析
    let mut engine = Engine::with_sandbox(sandbox.clone());
    if let Some(root) = import_root {
        engine.set_import_root(root.to_path_buf());
    }
    let val = engine.eval_file_diagnostic(path)?;
    if !val.is_nil() {
        println!("{val}");
    }
    Ok(())
}

fn run_code(source: &str, sandbox: &Sandbox) -> Result<(), Box<dyn std::error::Error>> {
    let mut engine = Engine::with_sandbox(sandbox.clone());
    // T1.8：用带位置的求值入口，运行时错误也能给出 line:col
    let val = engine.eval_diagnostic(source)?;
    if !val.is_nil() {
        println!("{val}");
    }
    Ok(())
}

/// `ada check <file>`：只做静态检查（语法 + 编译），**不执行**脚本（T7.2）。
///
/// 编辑器与 CI 用它做"这段代码能不能编译"的门禁；成功后保持安静（Unix 惯例）。
fn check_file(path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let source = fs::read_to_string(path)?;
    Engine::check(&source)?;
    Ok(())
}

fn dump_bytecode(path: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let source = fs::read_to_string(path)?;
    let mut parser = Parser::from_source(&source)?;
    let program = parser.parse_program()?;
    let proto = Compiler::compile_program(&program)?;

    println!("=== Bytecode for {} ===", path.display());
    println!("Registers: {}", proto.register_count);
    println!("Constants: {:?}", proto.constants);
    println!("Names: {:?}", proto.names);
    println!("Instructions:");
    for (i, inst) in proto.instructions.iter().enumerate() {
        println!("  {:04} {:?}", i, inst);
    }
    for child in &proto.child_protos {
        println!("\n--- Function {} (Arity: {}) ---", child.name, child.arity);
        for (i, inst) in child.instructions.iter().enumerate() {
            println!("  {:04} {:?}", i, inst);
        }
    }
    Ok(())
}

fn run_repl(sandbox: &Sandbox) -> Result<(), Box<dyn std::error::Error>> {
    println!("Ada-D 0.1.0 Interactive REPL");
    println!("Type 'exit' or press Ctrl+D to quit.\n");

    let mut rl = DefaultEditor::new()?;
    let mut engine = Engine::with_sandbox(sandbox.clone());

    loop {
        let readline = rl.readline("ada> ");
        match readline {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if trimmed == "exit" || trimmed == "quit" {
                    break;
                }
                rl.add_history_entry(trimmed)?;
                match engine.eval_value(trimmed) {
                    Ok(val) => {
                        if !val.is_nil() {
                            println!("{val}");
                        }
                    }
                    Err(err) => {
                        eprintln!("Error: {err}");
                    }
                }
            }
            Err(ReadlineError::Interrupted) | Err(ReadlineError::Eof) => {
                println!("\nGoodbye!");
                break;
            }
            Err(err) => {
                eprintln!("Error: {err}");
                break;
            }
        }
    }

    Ok(())
}
