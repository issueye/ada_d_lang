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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let sandbox = if cli.strict {
        Sandbox::strict()
    } else {
        // CLI 是本地受信工具：默认 standard（time/sys/env 白名单/sleep 可用），
        // 但对宿主环境变量仍然只放行显式白名单。
        Sandbox::standard()
    };

    if let Some(code) = cli.eval {
        run_code(&code, &sandbox)?;
        return Ok(());
    }

    match cli.command {
        Some(Commands::Run { file }) => run_file(&file, &sandbox)?,
        Some(Commands::Eval { code }) => run_code(&code, &sandbox)?,
        Some(Commands::Dump { file }) => dump_bytecode(&file)?,
        Some(Commands::Repl) => run_repl(&sandbox)?,
        None => {
            if let Some(file) = cli.file {
                run_file(&file, &sandbox)?;
            } else {
                run_repl(&sandbox)?;
            }
        }
    }

    Ok(())
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
