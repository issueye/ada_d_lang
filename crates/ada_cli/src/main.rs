use ada_bytecode::Compiler;
use ada_engine::{Engine, Sandbox};
use ada_parser::Parser;
use clap::{Parser as ClapParser, Subcommand};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::fs;
use std::path::PathBuf;

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

    // T0.6：统一错误格式 `[file:]line:col: error: <消息>`（有位置时）或 `<file>: error: <消息>`。
    // 自行捕获错误而不依赖 `main -> Result` 的 Debug 输出。
    let source_name = cli
        .file
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "<eval>".to_string());

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
        Some(Commands::Run { file }) => run_file(file, sandbox),
        Some(Commands::Eval { code }) => run_code(code, sandbox),
        Some(Commands::Dump { file }) => dump_bytecode(file),
        Some(Commands::Repl) => run_repl(sandbox),
        None => {
            if let Some(file) = &cli.file {
                run_file(file, sandbox)
            } else {
                run_repl(sandbox)
            }
        }
    }
}

/// 把错误渲染成稳定的、面向用户与工具的单行格式（T0.6）。
///
/// - 词法/语法错误带 `line:col`（`ParseError::position`）；
/// - 编译/运行时错误当前**没有** span（T1.8 待补），因此只给文件名与消息；
///   格式保持 `file: error: <消息>`，工具可按 `:` 切分而不必区分是否带位置。
fn render_error(source_name: &str, err: &(dyn std::error::Error + 'static)) -> String {
    if let Some(engine_err) = err.downcast_ref::<ada_engine::Error>() {
        match engine_err {
            ada_engine::Error::Parse(parse_err) => {
                let (line, col) = parse_err.position();
                return format!(
                    "{source_name}:{line}:{col}: error: {}",
                    parse_message(parse_err)
                );
            }
            ada_engine::Error::Compile(e) => {
                return format!("{source_name}: error: {e}");
            }
            ada_engine::Error::Vm(e) => {
                return format!("{source_name}: error: {e}");
            }
            ada_engine::Error::Conversion(msg) => {
                return format!("{source_name}: error: {msg}");
            }
            ada_engine::Error::ModuleConflict(name) => {
                return format!("{source_name}: error: module '{name}' is already registered");
            }
        }
    }
    format!("{source_name}: error: {err}")
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

fn run_file(path: &PathBuf, sandbox: &Sandbox) -> Result<(), Box<dyn std::error::Error>> {
    let source = fs::read_to_string(path)?;
    run_code(&source, sandbox)?;
    Ok(())
}

fn run_code(source: &str, sandbox: &Sandbox) -> Result<(), Box<dyn std::error::Error>> {
    let mut engine = Engine::with_sandbox(sandbox.clone());
    let val = engine.eval_value(source)?;
    if !val.is_nil() {
        println!("{val}");
    }
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
