mod backend;
mod compiler;
use compiler::Compiler;
mod ir;
mod loader;
mod runtime;
mod stdlib;
pub mod types;
mod vm;

use swc_ecma_parser::{Syntax, TsSyntax};

use crate::ir::IrModule;
use crate::loader::BytecodeDecoder;
use crate::vm::VM;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
#[cfg(test)]
mod tests;

/// Default path for the prelude file
const PRELUDE_PATH: &str = "std/prelude.tscl";

/// Bootstrap compiler files (loaded in order when running bootstrap tests)
const BOOTSTRAP_FILES: &[&str] = &[
    "bootstrap/types.tscl",
    "bootstrap/lexer.tscl",
    "bootstrap/parser.tscl",
    "bootstrap/emitter.tscl",
    "bootstrap/ir.tscl",
    "bootstrap/ir_builder.tscl",
    "bootstrap/codegen.tscl",
    "bootstrap/pipeline.tscl",
];

/// Modular compiler files (loaded in dependency order)
const MODULAR_COMPILER_FILES: &[&str] = &[
    // Level 1: No dependencies
    "compiler/lexer/token.tscl",
    "compiler/ast/types.tscl",
    "compiler/ir/mod.tscl",
    // Level 2: Depends on level 1
    "compiler/lexer/mod.tscl",
    "compiler/parser/expr.tscl",
    "compiler/parser/stmt.tscl",
    "compiler/ir/builder.tscl",
    "compiler/passes/typecheck.tscl",
    "compiler/passes/opt.tscl",
    "compiler/passes/borrow_ck.tscl",
    // Level 3: Depends on level 2
    "compiler/parser/mod.tscl",
    "compiler/passes/mod.tscl",
    "compiler/codegen/mod.tscl",
    // Level 4: Depends on level 3
    "compiler/codegen/emitter.tscl",
    // Level 5: Backend modules
    "compiler/backend/llvm/runtime.tscl",
    "compiler/backend/llvm/types.tscl",
    "compiler/backend/llvm/mod.tscl",
    // Level 6: Top-level pipeline
    "compiler/pipeline.tscl",
];

/// Helper to load and run a script file
fn load_and_run_script(
    vm: &mut VM,
    compiler: &mut Compiler,
    path: &str,
    append: bool,
) -> Result<(), String> {
    let source = fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {}", path, e))?;

    // Determine syntax based on file extension
    let syntax = if path.ends_with(".ts") || path.ends_with(".tsx") {
        // Enable decorators for TypeScript
        let ts_syntax = TsSyntax {
            decorators: true,
            tsx: path.ends_with(".tsx"),
            ..Default::default()
        };
        Some(Syntax::Typescript(ts_syntax))
    } else if path.ends_with(".js") || path.ends_with(".jsx") {
        Some(Syntax::Es(Default::default()))
    } else {
        // Default to TypeScript with decorators for .tscl files
        let ts_syntax = TsSyntax {
            decorators: true,
            ..Default::default()
        };
        Some(Syntax::Typescript(ts_syntax))
    };

    let bytecode = compiler
        .compile_with_syntax(&source, syntax)
        .map_err(|e| format!("Failed to compile {}: {}", path, e))?;
    let bytecode_len = bytecode.len();

    if append {
        let offset = vm.append_program(bytecode);
        println!("  {} ({} ops at offset {})", path, bytecode_len, offset);
    } else {
        let path_buf = PathBuf::from(path);
        vm.load_program_with_path(bytecode, path_buf);
        println!("  {} ({} ops)", path, bytecode_len);
    }

    vm.run_until_halt();
    Ok(())
}

/// Load and run a pre-compiled bytecode file
fn run_binary_file(vm: &mut VM, path: &str) -> Result<(), String> {
    let bytes =
        fs::read(path).map_err(|e| format!("Failed to read binary file {}: {}", path, e))?;

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
        eprintln!("Usage: {} <command> [args...]", args[0]);
        eprintln!("Commands:");
        eprintln!("  check <filename>     Check a .tscl file for errors (for LSP)");
        eprintln!("  ir <filename>        Dump SSA IR for a .tscl file");
        eprintln!("  jit <filename>       Run a .tscl file with JIT compilation");
        eprintln!("  bench <filename>     Benchmark VM vs JIT for a .tscl file");
        eprintln!("  build [options] <filename>  Build a .tscl file to native binary");
        eprintln!("  <filename>           Run a .tscl file (VM interpreter)");
        eprintln!("  --run-binary <file>  Run a bytecode file (.bc)");
        eprintln!("");
        eprintln!("Build options:");
        eprintln!("  --backend <llvm|cranelift>  Choose code generator (default: llvm)");
        eprintln!("  --output <file>, -o <file>  Output file name");
        eprintln!("  --release                      Optimize with ThinLTO");
        eprintln!("  --dist                         Full LTO for maximum performance");
        eprintln!("  --debug                        No optimization, debug info");
        eprintln!("  --format <exe|lib|dylib|obj>   Output format");
        eprintln!("  --emit-ir                      Emit SSA IR to .ir file");
        eprintln!("  --emit-llvm                    Emit LLVM IR to .ll file");
        eprintln!("  --emit-obj                     Emit object file to .o file");
        eprintln!("  --verify-ir                    Validate IR and exit");
        return;
    }

    let command = &args[1];

    // Handle "check" command for LSP diagnostics
    if command == "check" {
        if args.len() < 3 {
            eprintln!("Usage: {} check <filename>", args[0]);
            std::process::exit(1);
        }
        let filename = &args[2];
        check_file(filename);
        return;
    }

    // Handle "ir" command to dump SSA IR
    if command == "ir" {
        if args.len() < 3 {
            eprintln!("Usage: {} ir <filename>", args[0]);
            std::process::exit(1);
        }
        let filename = &args[2];
        dump_ir(filename);
        return;
    }

    // Handle "jit" command for JIT compilation
    if command == "jit" {
        if args.len() < 3 {
            eprintln!("Usage: {} jit <filename>", args[0]);
            std::process::exit(1);
        }
        let filename = &args[2];
        run_jit(filename);
        return;
    }

    // Handle "bench" command for benchmarking
    if command == "bench" {
        if args.len() < 3 {
            eprintln!("Usage: {} bench <filename>", args[0]);
            std::process::exit(1);
        }
        let filename = &args[2];
        run_benchmark(filename);
        return;
    }

    // Handle "build" command for AOT compilation
    if command == "build" {
        build_file(&args[2..]);
        return;
    }

    let filename = command;

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
    let is_bootstrap = filename.contains("bootstrap/") || filename.contains("tests/");
    let is_modular_compiler = filename.contains("compiler/") && !filename.contains("bootstrap/");

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

    if is_modular_compiler {
        println!("Loading modular compiler modules...");
        for modular_file in MODULAR_COMPILER_FILES {
            // Skip the main file being run if it's in the list
            if modular_file == &filename {
                continue;
            }
            if Path::new(modular_file).exists() {
                if let Err(e) = load_and_run_script(&mut vm, &mut compiler, modular_file, true) {
                    eprintln!("{}", e);
                    return;
                }
            } else {
                eprintln!("Warning: Modular compiler file not found: {}", modular_file);
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

    // Determine syntax based on file extension
    let syntax = if filename.ends_with(".ts") || filename.ends_with(".tsx") {
        let ts_syntax = TsSyntax {
            decorators: true,
            tsx: filename.ends_with(".tsx"),
            ..Default::default()
        };
        Some(Syntax::Typescript(ts_syntax))
    } else if filename.ends_with(".js") || filename.ends_with(".jsx") {
        Some(Syntax::Es(Default::default()))
    } else {
        // Default to TypeScript with decorators for .tscl files
        let ts_syntax = TsSyntax {
            decorators: true,
            ..Default::default()
        };
        Some(Syntax::Typescript(ts_syntax))
    };

    match compiler.compile_with_syntax(&main_source, syntax) {
        Ok(main_bytecode) => {
            let offset = vm.append_program(main_bytecode);
            // Update the current module path to the main script for relative imports
            vm.set_current_module_path(PathBuf::from(filename));

            // Set script arguments (__args__) for the script
            // Arguments after the filename are passed to the script
            let script_args: Vec<String> = args[2..].to_vec();
            vm.set_script_args(script_args);

            println!("Running from offset {}...", offset);
            vm.run_event_loop();
        }
        Err(e) => {
            eprintln!("Compilation failed: {}", e);
        }
    }
}

/// Dump SSA IR for a file
fn dump_ir(filename: &str) {
    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to read {}: {}", filename, e);
            std::process::exit(1);
        }
    };

    // Determine syntax based on file extension
    let syntax = if filename.ends_with(".ts") || filename.ends_with(".tsx") {
        let ts_syntax = TsSyntax {
            decorators: true,
            tsx: filename.ends_with(".tsx"),
            ..Default::default()
        };
        Some(Syntax::Typescript(ts_syntax))
    } else if filename.ends_with(".js") || filename.ends_with(".jsx") {
        Some(Syntax::Es(Default::default()))
    } else {
        // Default to TypeScript with decorators for .tscl files
        let ts_syntax = TsSyntax {
            decorators: true,
            ..Default::default()
        };
        Some(Syntax::Typescript(ts_syntax))
    };

    let mut compiler = Compiler::new();
    let bytecode = match compiler.compile_with_syntax(&source, syntax) {
        Ok(bc) => bc,
        Err(e) => {
            eprintln!("Compilation failed: {}", e);
            std::process::exit(1);
        }
    };

    println!("=== Bytecode ({} instructions) ===", bytecode.len());
    for (i, op) in bytecode.iter().enumerate() {
        println!("  [{:4}] {:?}", i, op);
    }
    println!();

    // Lower to SSA IR
    match ir::lower::lower_module(&bytecode) {
        Ok(mut module) => {
            println!("=== SSA IR (before optimization) ===");
            println!("{}", module);

            // Run type inference and specialization
            ir::typecheck::typecheck_module(&mut module);
            println!("=== SSA IR (after type inference) ===");
            println!("{}", module);

            // Run optimizations
            ir::opt::optimize_module(&mut module);
            println!("=== SSA IR (after optimization) ===");
            println!("{}", module);
        }
        Err(e) => {
            eprintln!("IR lowering failed: {}", e);
            std::process::exit(1);
        }
    }
}

/// Check a file for errors without running it
fn check_file(filename: &str) {
    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}:1:1: Failed to read file: {}", filename, e);
            std::process::exit(1);
        }
    };

    // Determine syntax based on file extension
    let syntax = if filename.ends_with(".ts") || filename.ends_with(".tsx") {
        let ts_syntax = TsSyntax {
            decorators: true,
            tsx: filename.ends_with(".tsx"),
            ..Default::default()
        };
        Some(Syntax::Typescript(ts_syntax))
    } else if filename.ends_with(".js") || filename.ends_with(".jsx") {
        Some(Syntax::Es(Default::default()))
    } else {
        // Default to TypeScript with decorators for .tscl files
        let ts_syntax = TsSyntax {
            decorators: true,
            ..Default::default()
        };
        Some(Syntax::Typescript(ts_syntax))
    };

    let mut compiler = Compiler::new();
    match compiler.compile_with_syntax(&source, syntax) {
        Ok(_) => {
            // Success - no errors
            std::process::exit(0);
        }
        Err(e) => {
            // Parse error message to extract line/column if possible
            // Format: "Parsing error: ..." or "BORROW ERROR: ..." or "LIFETIME ERROR: ..."

            // Try to find the line number from the error
            let mut line_num = 1;
            let col_num = 1;

            // Check if error contains line information from SWC
            if e.contains("error at") || e.contains("line") {
                // Try to extract line number from error message
                // This is a simple heuristic - SWC errors might have different formats
                let lines: Vec<&str> = source.lines().collect();
                for (i, _line) in lines.iter().enumerate() {
                    if e.contains(&format!("line {}", i + 1))
                        || e.contains(&format!("{}:{}", filename, i + 1))
                    {
                        line_num = i + 1;
                        break;
                    }
                }
            }

            // Output in format: filename:line:col: message
            eprintln!("{}:{}:{}: {}", filename, line_num, col_num, e);
            std::process::exit(1);
        }
    }
}

/// Run a file using JIT compilation
fn run_jit(filename: &str) {
    use crate::backend::{BackendConfig, jit::JitRuntime};

    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to read {}: {}", filename, e);
            std::process::exit(1);
        }
    };

    // Determine syntax based on file extension
    let syntax = if filename.ends_with(".ts") || filename.ends_with(".tsx") {
        let ts_syntax = TsSyntax {
            decorators: true,
            tsx: filename.ends_with(".tsx"),
            ..Default::default()
        };
        Some(Syntax::Typescript(ts_syntax))
    } else if filename.ends_with(".js") || filename.ends_with(".jsx") {
        Some(Syntax::Es(Default::default()))
    } else {
        // Default to TypeScript with decorators for .tscl files
        let ts_syntax = TsSyntax {
            decorators: true,
            ..Default::default()
        };
        Some(Syntax::Typescript(ts_syntax))
    };

    // Compile to bytecode
    let mut compiler = Compiler::new();
    let bytecode = match compiler.compile_with_syntax(&source, syntax) {
        Ok(bc) => bc,
        Err(e) => {
            eprintln!("Compilation failed: {}", e);
            std::process::exit(1);
        }
    };

    println!("=== Bytecode ({} instructions) ===", bytecode.len());
    for (i, op) in bytecode.iter().enumerate() {
        println!("[{:3}] {:?}", i, op);
    }

    // Lower to SSA IR (using lower_module to extract all functions)
    match ir::lower::lower_module(&bytecode) {
        Ok(mut module) => {
            // Show extracted functions
            if module.functions.len() > 1 {
                println!("\n=== Extracted Functions ===");
                for (i, func) in module.functions.iter().enumerate() {
                    if func.name != "main" {
                        println!("  [{}] {} ({} blocks)", i, func.name, func.blocks.len());
                    }
                }
            }

            // Run type inference
            ir::typecheck::typecheck_module(&mut module);

            // Run optimizations
            ir::opt::optimize_module(&mut module);

            println!("\n=== SSA IR (optimized) ===");
            println!("{}", module);

            // JIT compile
            println!("\n=== JIT Compilation ===");
            let config = BackendConfig::default();
            let mut runtime = match JitRuntime::new(&config) {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("Failed to create JIT runtime: {}", e);
                    std::process::exit(1);
                }
            };

            match runtime.compile(&module) {
                Ok(()) => {
                    println!("JIT compilation successful!");

                    // Show compiled functions
                    let funcs = runtime.get_all_funcs();
                    println!("Compiled {} functions:", funcs.len());
                    for name in funcs.keys() {
                        println!("  - {}", name);
                    }

                    // Try to call main
                    println!("\n=== Execution ===");
                    match runtime.call_main() {
                        Ok(result) => {
                            println!("Result: {:?}", result);
                        }
                        Err(e) => {
                            eprintln!("Execution failed: {}", e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("JIT compilation failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("IR lowering failed: {}", e);
            std::process::exit(1);
        }
    }
}

/// Run a benchmark comparing VM vs JIT performance
fn run_benchmark(filename: &str) {
    use crate::backend::{BackendConfig, jit::JitRuntime};
    use std::time::Instant;

    let source = match fs::read_to_string(filename) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to read {}: {}", filename, e);
            std::process::exit(1);
        }
    };

    // Determine syntax based on file extension
    let syntax = if filename.ends_with(".ts") || filename.ends_with(".tsx") {
        let ts_syntax = TsSyntax {
            decorators: true,
            tsx: filename.ends_with(".tsx"),
            ..Default::default()
        };
        Some(Syntax::Typescript(ts_syntax))
    } else if filename.ends_with(".js") || filename.ends_with(".jsx") {
        Some(Syntax::Es(Default::default()))
    } else {
        // Default to TypeScript with decorators for .tscl files
        let ts_syntax = TsSyntax {
            decorators: true,
            ..Default::default()
        };
        Some(Syntax::Typescript(ts_syntax))
    };

    // Compile to bytecode
    let mut compiler = Compiler::new();
    let bytecode = match compiler.compile_with_syntax(&source, syntax) {
        Ok(bc) => bc,
        Err(e) => {
            eprintln!("Compilation failed: {}", e);
            std::process::exit(1);
        }
    };

    println!("=== Benchmark: {} ===\n", filename);

    const ITERATIONS: u32 = 100;

    // Benchmark VM (without prelude for fair comparison)
    // Note: For VM, we replace top-level Return with Halt to keep the frame intact
    println!("VM Interpreter ({} iterations):", ITERATIONS);
    let mut vm_bytecode = bytecode.clone();
    // Replace the last Return before Halt with just letting execution continue
    for i in 0..vm_bytecode.len() {
        if matches!(vm_bytecode[i], crate::vm::opcodes::OpCode::Return)
            && i + 1 < vm_bytecode.len()
            && matches!(vm_bytecode[i + 1], crate::vm::opcodes::OpCode::Halt)
        {
            // The return value is already on the stack - just skip to Halt
            vm_bytecode[i] = crate::vm::opcodes::OpCode::Halt;
            break;
        }
    }

    let mut vm_results = Vec::new();
    let vm_start = Instant::now();
    for _ in 0..ITERATIONS {
        let mut vm = VM::new_bare(); // Use bare VM without stdlib for benchmark
        vm.load_program(vm_bytecode.clone());
        vm.run_until_halt();
        // Get the result (top of stack or undefined)
        let result = vm
            .stack
            .pop()
            .unwrap_or(crate::vm::value::JsValue::Undefined);
        vm_results.push(result);
    }
    let vm_duration = vm_start.elapsed();
    println!("  Total time: {:?}", vm_duration);
    println!("  Per iteration: {:?}", vm_duration / ITERATIONS);
    if let Some(result) = vm_results.first() {
        println!("  Result: {:?}", result);
    }

    // Benchmark JIT
    println!("\nJIT Compilation:");

    // Lower to IR
    let module = match ir::lower::lower_module(&bytecode) {
        Ok(mut m) => {
            ir::typecheck::typecheck_module(&mut m);
            ir::opt::optimize_module(&mut m);
            m
        }
        Err(e) => {
            eprintln!("  IR lowering failed: {}", e);
            return;
        }
    };

    // JIT compile
    let config = BackendConfig::default();
    let mut runtime = match JitRuntime::new(&config) {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("  Failed to create JIT runtime: {}", e);
            return;
        }
    };

    let compile_start = Instant::now();
    if let Err(e) = runtime.compile(&module) {
        eprintln!("  JIT compilation failed: {}", e);
        return;
    }
    let compile_duration = compile_start.elapsed();
    println!("  Compilation time: {:?}", compile_duration);

    // Run JIT
    println!("\nJIT Execution ({} iterations):", ITERATIONS);
    let mut jit_results = Vec::new();
    let jit_start = Instant::now();
    for _ in 0..ITERATIONS {
        match runtime.call_main() {
            Ok(result) => jit_results.push(result),
            Err(e) => {
                eprintln!("  Execution error: {}", e);
                break;
            }
        }
    }
    let jit_duration = jit_start.elapsed();
    println!("  Total time: {:?}", jit_duration);
    println!("  Per iteration: {:?}", jit_duration / ITERATIONS);
    if let Some(result) = jit_results.first() {
        println!("  Result: {:?}", result);
    }

    // Summary
    println!("\n=== Summary ===");
    let vm_per_iter = vm_duration.as_nanos() as f64 / ITERATIONS as f64;
    let jit_per_iter = jit_duration.as_nanos() as f64 / ITERATIONS as f64;
    let speedup = vm_per_iter / jit_per_iter;

    println!("VM:  {:>10.2} µs/iter", vm_per_iter / 1000.0);
    println!("JIT: {:>10.2} µs/iter", jit_per_iter / 1000.0);
    println!("JIT compilation: {:>10.2} µs", compile_duration.as_micros());

    if speedup > 1.0 {
        println!("\nJIT is {:.2}x faster than VM", speedup);
    } else {
        println!("\nVM is {:.2}x faster than JIT", 1.0 / speedup);
    }

    // Break-even analysis
    let break_even = compile_duration.as_nanos() as f64 / (vm_per_iter - jit_per_iter).max(1.0);
    if speedup > 1.0 {
        println!("Break-even point: {:.0} iterations", break_even);
    }
}

/// Build a file to native binary using LLVM AOT compilation
fn build_file(args: &[String]) {
    use crate::backend::{
        BackendConfig, BackendKind, LtoMode, OptLevel,
        aot::{AotCompiler, AotOptions, OutputFormat},
    };

    let mut filenames = Vec::new();
    let mut output = None;
    let mut backend = BackendKind::LlvmAot;
    let mut opt_level = OptLevel::None; // Default to dev mode
    let mut format = OutputFormat::Executable;
    let mut lto_mode = LtoMode::None;
    let mut emit_ir = false;
    let mut emit_llvm = false;
    let mut emit_obj = false;
    let mut verify_ir = false;

    // Parse arguments
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--backend" | "-b" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --backend requires a value");
                    std::process::exit(1);
                }
                backend = match args[i].as_str() {
                    "llvm" => BackendKind::LlvmAot,
                    "cranelift" => BackendKind::CraneliftAot,
                    _ => {
                        eprintln!("Error: Unknown backend: {}", args[i]);
                        std::process::exit(1);
                    }
                };
            }
            "--output" | "-o" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --output requires a value");
                    std::process::exit(1);
                }
                output = Some(args[i].clone());
            }
            "--release" => {
                opt_level = OptLevel::SpeedAndSize;
                lto_mode = LtoMode::Thin; // Release uses ThinLTO
            }
            "--dist" => {
                opt_level = OptLevel::SpeedAndSize;
                lto_mode = LtoMode::Full; // Dist uses Full LTO
            }
            "--debug" => {
                opt_level = OptLevel::None;
                lto_mode = LtoMode::None;
            }
            "--format" => {
                i += 1;
                if i >= args.len() {
                    eprintln!("Error: --format requires a value");
                    std::process::exit(1);
                }
                format = match args[i].as_str() {
                    "exe" | "executable" => OutputFormat::Executable,
                    "lib" | "static" => OutputFormat::StaticLib,
                    "dylib" | "shared" => OutputFormat::SharedLib,
                    "obj" | "object" => OutputFormat::Object,
                    _ => {
                        eprintln!("Error: Unknown format: {}", args[i]);
                        std::process::exit(1);
                    }
                };
            }
            "--emit-ir" => {
                emit_ir = true;
            }
            "--emit-llvm" => {
                emit_llvm = true;
            }
            "--emit-obj" => {
                emit_obj = true;
            }
            "--verify-ir" => {
                verify_ir = true;
            }
            _ => {
                if !args[i].starts_with('-') {
                    filenames.push(args[i].clone());
                } else {
                    eprintln!("Error: Unknown option: {}", args[i]);
                    std::process::exit(1);
                }
            }
        }
        i += 1;
    }

    if filenames.is_empty() {
        eprintln!("Error: No input file specified");
        eprintln!(
            "Usage: {} build [--backend llvm] [--output <file>] [--release|--dist] [--emit-ir|--emit-llvm|--emit-obj] [--verify-ir] <filename>...",
            env::args().next().unwrap()
        );
        eprintln!("Emission flags:");
        eprintln!("  --emit-ir       Output SSA IR to file.ir");
        eprintln!("  --emit-llvm     Output LLVM IR to file.ll");
        eprintln!("  --emit-obj      Output object file to file.o");
        eprintln!("  --verify-ir     Validate SSA IR and exit");
        std::process::exit(1);
    }

    // Compile all source files to IR modules
    let mut modules = Vec::new();
    let mut compiler = Compiler::new();

    for filename in &filenames {
        // Read source file
        let source = match fs::read_to_string(filename) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Failed to read {}: {}", filename, e);
                std::process::exit(1);
            }
        };

        // Determine syntax
        let syntax = if filename.ends_with(".ts") || filename.ends_with(".tsx") {
            let ts_syntax = TsSyntax {
                decorators: true,
                tsx: filename.ends_with(".tsx"),
                ..Default::default()
            };
            Some(Syntax::Typescript(ts_syntax))
        } else if filename.ends_with(".js") || filename.ends_with(".jsx") {
            Some(Syntax::Es(Default::default()))
        } else {
            // Default to TypeScript with decorators for .tscl files
            let ts_syntax = TsSyntax {
                decorators: true,
                ..Default::default()
            };
            Some(Syntax::Typescript(ts_syntax))
        };

        // Compile to bytecode
        let bytecode = match compiler.compile_with_syntax(&source, syntax) {
            Ok(bc) => bc,
            Err(e) => {
                eprintln!("Compilation failed for {}: {}", filename, e);
                std::process::exit(1);
            }
        };

        // Lower to SSA IR
        let mut module = match ir::lower::lower_module(&bytecode) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("IR lowering failed for {}: {}", filename, e);
                std::process::exit(1);
            }
        };

        // Run type inference and optimizations
        ir::typecheck::typecheck_module(&mut module);
        ir::opt::optimize_module(&mut module);

        // Verify IR if requested
        if verify_ir {
            match ir::verify::verify_module(&module) {
                Ok(()) => {
                    println!("IR verification passed for {}", filename);
                }
                Err(errors) => {
                    eprintln!("IR verification failed for {}:", filename);
                    for error in &errors {
                        eprintln!("  - {}", error);
                    }
                    std::process::exit(1);
                }
            }
            continue; // Skip to next file
        }

        // Emit IR if requested
        if emit_ir {
            let ir_output = Path::new(filename)
                .file_stem()
                .map(|s| Path::new(filename).with_extension("ir").to_path_buf())
                .unwrap_or_else(|| PathBuf::from("output.ir"));

            match ir::format::write_ir_to_file(&module, &ir_output) {
                Ok(()) => {
                    println!("IR written to: {}", ir_output.display());
                }
                Err(e) => {
                    eprintln!("Failed to write IR: {}", e);
                    std::process::exit(1);
                }
            }
        }

        modules.push(module);
    }

    // If only verification was requested, we're done
    if verify_ir {
        println!("All files verified successfully.");
        return;
    }

    // If only IR emission was requested, we're done
    if emit_ir && !emit_llvm && !emit_obj {
        return;
    }

    // AOT compile
    if filenames.len() == 1 {
        println!("Compiling {} to native binary...", filenames[0]);
    } else {
        println!("Compiling {} files to native binary...", filenames.len());
    }

    let config = BackendConfig {
        kind: backend,
        opt_level,
        debug_info: opt_level == OptLevel::None,
        bounds_check: true,
        lto_mode,
    };

    let mut aot = AotCompiler::new(&config);
    let mut options = AotOptions::default();
    options.format = format;
    options.lto_mode = lto_mode;
    aot = aot.with_options(options);

    // Compile all modules (with LTO support if enabled)
    let module_refs: Vec<&IrModule> = modules.iter().collect();

    // Determine output path
    let output_path = output.unwrap_or_else(|| {
        // Use first filename as default output name
        Path::new(&filenames[0])
            .file_stem()
            .unwrap()
            .to_string_lossy()
            .to_string()
    });

    // Emit object file if requested
    if emit_obj {
        let obj_output = Path::new(&output_path).with_extension("o");
        match aot.compile_modules_to_object(&module_refs, &obj_output) {
            Ok(()) => {
                println!("Object file written to: {}", obj_output.display());
            }
            Err(e) => {
                eprintln!("Object file compilation failed: {}", e);
                std::process::exit(1);
            }
        }
    }

    // Emit LLVM IR if requested
    if emit_llvm {
        let llvm_output = Path::new(&output_path).with_extension("ll");
        match aot.compile_modules_to_llvm_ir(&module_refs, &llvm_output) {
            Ok(()) => {
                println!("LLVM IR written to: {}", llvm_output.display());
            }
            Err(e) => {
                eprintln!("LLVM IR emission failed: {}", e);
                std::process::exit(1);
            }
        }
    }

    // Full compilation to executable/library
    match aot.compile_modules_to_file(&module_refs, Path::new(&output_path)) {
        Ok(()) => {
            println!("Successfully compiled to: {}", output_path);
        }
        Err(e) => {
            eprintln!("AOT compilation failed: {}", e);
            std::process::exit(1);
        }
    }
}
