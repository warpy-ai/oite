mod compiler;
use compiler::Compiler;
mod loader;
mod stdlib;
mod vm;

use crate::loader::BytecodeDecoder;
use crate::vm::VM;
use std::env;
use std::fs;
use std::path::Path;
#[cfg(test)]
mod tests;

/// Default path for the prelude file
const PRELUDE_PATH: &str = "std/prelude.tscl";

/// Bootstrap compiler files (loaded in order when running bootstrap tests)
const BOOTSTRAP_FILES: &[&str] = &[
    "bootstrap/lexer.tscl",
    "bootstrap/parser.tscl",
    "bootstrap/emitter.tscl",
];

/// Helper to load and run a script file
fn load_and_run_script(vm: &mut VM, compiler: &mut Compiler, path: &str, append: bool) -> Result<(), String> {
    let source = fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {}", path, e))?;
    let bytecode = compiler.compile(&source).map_err(|e| format!("Failed to compile {}: {}", path, e))?;
    let bytecode_len = bytecode.len();

    if append {
        let offset = vm.append_program(bytecode);
        println!("  {} ({} ops at offset {})", path, bytecode_len, offset);
    } else {
        vm.load_program(bytecode);
        println!("  {} ({} ops)", path, bytecode_len);
    }

    vm.run_until_halt();
    Ok(())
}

/// Load and run a pre-compiled bytecode file
fn run_binary_file(vm: &mut VM, path: &str) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|e| format!("Failed to read binary file {}: {}", path, e))?;

    let mut decoder = BytecodeDecoder::new(&bytes);

    match decoder.decode_all() {
        Ok(program) => {
            println!("Loaded {} instructions from binary file", program.len());
            // Debug: print each instruction
            for (i, op) in program.iter().enumerate() {
                println!("  [{}] {:?}", i, op);
            }
            // Debug: check if console is in global frame
            if let Some(frame) = vm.call_stack.first() {
                println!("Global frame has {} locals", frame.locals.len());
                if frame.locals.contains_key("console") {
                    println!("  - console: found");
                } else {
                    println!("  - console: NOT FOUND!");
                }
            }
            let offset = vm.append_program(program);
            println!("Running from offset {}...", offset);
            vm.run_event_loop();
            println!("Execution complete.");
            Ok(())
        }
        Err(e) => Err(format!("Failed to decode bytecode: {}", e)),
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <filename> [--run-binary]", args[0]);
        eprintln!("  <filename>      Source file (.tscl) or bytecode file (.bc)");
        eprintln!("  --run-binary    Force interpretation as bytecode file");
        return;
    }
    let filename = &args[1];

    // Check if we should run in binary mode
    let run_binary = args.iter().any(|a| a == "--run-binary")
        || filename.ends_with(".bc")
        || filename.ends_with(".tscl.bc");

    let mut vm = VM::new();
    let mut compiler = Compiler::new();

    // Setup standard library
    vm.setup_stdlib();

    // Binary mode: load and run pre-compiled bytecode directly
    if run_binary {
        println!("Running bytecode file: {}", filename);
        if let Err(e) = run_binary_file(&mut vm, filename) {
            eprintln!("{}", e);
        }
        return;
    }

    // 1. Load and run prelude first (if exists)
    // This sets up global constants (OP, TOKEN, TYPE) and utility functions
    if Path::new(PRELUDE_PATH).exists() {
        println!("Loading prelude...");
        if let Err(e) = load_and_run_script(&mut vm, &mut compiler, PRELUDE_PATH, false) {
            eprintln!("{}", e);
            return;
        }
    }

    // 2. Check if this is a bootstrap file that needs the compiler modules
    let is_bootstrap = filename.contains("bootstrap/");

    if is_bootstrap {
        println!("Loading bootstrap compiler modules...");
        for bootstrap_file in BOOTSTRAP_FILES {
            if Path::new(bootstrap_file).exists() {
                if let Err(e) = load_and_run_script(&mut vm, &mut compiler, bootstrap_file, true) {
                    eprintln!("{}", e);
                    return;
                }
            } else {
                eprintln!("Warning: Bootstrap file not found: {}", bootstrap_file);
            }
        }
    }

    // 3. Load and run the main script
    println!("Loading main script: {}", filename);
    let main_source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to read {}: {}", filename, e);
            return;
        }
    };

    match compiler.compile(&main_source) {
        Ok(main_bytecode) => {
            let offset = vm.append_program(main_bytecode);
            println!("Running from offset {}...", offset);
            vm.run_event_loop();
        }
        Err(e) => {
            eprintln!("Compilation failed: {}", e);
        }
    }
}
