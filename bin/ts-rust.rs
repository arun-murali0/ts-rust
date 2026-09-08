use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ts_rust::TypeChecker;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.is_empty() || args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        return ExitCode::SUCCESS;
    }

    let Some(project_arg) = parse_project_arg(&args) else {
        eprintln!("error: expected --project <path-to-tsconfig.json>");
        print_help();
        return ExitCode::from(2);
    };

    let tsconfig_path = PathBuf::from(&project_arg);
    if !tsconfig_path.is_file() {
        eprintln!("error: no such tsconfig file: {}", tsconfig_path.display());
        return ExitCode::from(2);
    }

    let Some(project_root) = tsconfig_path.parent() else {
        eprintln!(
            "error: could not determine a project root from {}",
            tsconfig_path.display()
        );
        return ExitCode::from(2);
    };

    let mut source_files = Vec::new();
    collect_ts_files(project_root, &mut source_files);
    source_files.sort();

    if source_files.is_empty() {
        eprintln!(
            "warning: no .ts/.tsx files found under {}",
            project_root.display()
        );
        return ExitCode::SUCCESS;
    }

    let checker = TypeChecker::new();
    let mut error_count = 0usize;
    let mut warning_count = 0usize;
    let mut had_fatal = false;

    for file_path in &source_files {
        let source = match fs::read_to_string(file_path) {
            Ok(source) => source,
            Err(read_err) => {
                eprintln!("error: could not read {}: {read_err}", file_path.display());
                had_fatal = true;
                continue;
            }
        };

        let file_name = file_path.to_string_lossy().into_owned();
        match checker.check_source(&source, &file_name) {
            Ok(result) => {
                let line_index = ts_rust::LineIndex::new(&source);
                for diagnostic in &result.diagnostics {
                    match diagnostic.severity {
                        ts_rust::Severity::Error => error_count += 1,
                        ts_rust::Severity::Warning => warning_count += 1,
                    }
                    println!("{}", diagnostic.format_with_position(&line_index, &source));
                }
            }
            Err(parse_err) => {
                eprintln!("error: {file_name}: {parse_err}");
                had_fatal = true;
            }
        }
    }

    println!();
    println!(
        "Checked {} file(s): {} error(s), {} warning(s).",
        source_files.len(),
        error_count,
        warning_count
    );

    if had_fatal {
        ExitCode::from(2)
    } else if error_count > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn parse_project_arg(args: &[String]) -> Option<String> {
    for (i, arg) in args.iter().enumerate() {
        if let Some(value) = arg.strip_prefix("--project=") {
            return Some(value.to_string());
        }
        if arg == "--project" {
            return args.get(i + 1).cloned();
        }
    }
    None
}

fn collect_ts_files(dir: &Path, out: &mut Vec<PathBuf>) {
    const SKIPPED_DIRS: &[&str] = &["node_modules", "dist", "build", "out", "coverage", ".git"];

    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        if path.is_dir() {
            if name.starts_with('.') || SKIPPED_DIRS.contains(&name) {
                continue;
            }
            collect_ts_files(&path, out);
            continue;
        }

        let is_declaration_file = name.ends_with(".d.ts");
        let is_ts_source = name.ends_with(".ts") || name.ends_with(".tsx");
        if is_ts_source && !is_declaration_file {
            out.push(path);
        }
    }
}

fn print_help() {
    println!(
        "ts-rust ? a from-scratch TypeScript type checker (experimental CLI)\n\n\
         USAGE:\n    \
         ts-rust --project <path-to-tsconfig.json>\n\n\
         OPTIONS:\n    \
         --project <path>    Locate the project root from a tsconfig.json path.\n                         \
         Its contents are not parsed; every .ts/.tsx file under\n                         \
         that directory is checked directly. See src/bin/ts-rust.rs\n                         \
         for the exact scoping.\n    \
         -h, --help          Show this message.\n\n\
         EXIT CODES:\n    \
         0    no errors (warnings may still have been printed)\n    \
         1    one or more type errors were reported\n    \
         2    a file could not be parsed or read at all"
    );
}
