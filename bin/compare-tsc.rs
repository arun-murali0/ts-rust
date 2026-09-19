//! Compares ts-rust's own diagnostics against a real `tsc` run, in-process.
//!
//! Unlike scripts/harness.sh and scripts/compare-local.sh, this binary never
//! shells out to ts-rust as a subprocess — it calls `TypeChecker::check_source`
//! directly, the same entry point bin/ts-rust.rs uses, and only shells out to
//! `tsc` itself (there is no Rust API for that side of the comparison).
//!
//! Comparison is by (line, severity), not by message text. ts-rust's
//! `Diagnostic` currently carries no TS#### error code and writes its own
//! message text, so comparing message strings verbatim against tsc's wording
//! would report near-constant false mismatches. Position + severity is the
//! honest signal available today: did ts-rust flag an error where tsc did,
//! and nowhere tsc didn't. Full messages are still printed on a mismatch, as
//! context for a human, not as part of the pass/fail check.
//!
//! Fixtures live under tests/tsc-conformance/, a directory dedicated to this
//! comparison. tests/fixtures/ is NOT used here: several of those fixtures
//! deliberately exercise this checker's own recovery behavior (for example,
//! a class with one unresolvable member being treated as wholly unsupported)
//! and are expected to diverge from tsc, so mixing them in would produce
//! constant, meaningless failures.

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use ts_rust::{LineIndex, Severity, TypeChecker};

const REFERENCE_FILE: &str = "tests/tsc-reference.txt";
const CONFORMANCE_DIR: &str = "tests/tsc-conformance";
const TSC_BIN_ENV: &str = "TSC_BIN";

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ComparablePosition {
    line: u32,
    is_error: bool,
}

struct SideDiagnostics {
    positions: Vec<ComparablePosition>,
    // Kept only for human-readable output on a mismatch, never compared.
    full_lines: Vec<String>,
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<bool, String> {
    let Some(options) = parse_options()? else {
        return Ok(true);
    };

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let expected_version = read_reference_version(&root.join(REFERENCE_FILE))?;
    let tsc_bin = options.tsc_bin.clone();
    verify_tsc_version(&tsc_bin, &expected_version)?;

    let conformance_root = root.join(CONFORMANCE_DIR);
    let mut cases = discover_cases(&conformance_root)?;
    if let Some(case) = &options.case {
        cases.retain(|path| case_matches(path, case));
        if cases.is_empty() {
            return Err(format!("no conformance fixture matches '{case}'"));
        }
    }
    if cases.is_empty() {
        return Err(format!(
            "no .ts fixtures found under {}",
            conformance_root.display()
        ));
    }

    let checker = TypeChecker::new();
    let mut mismatches = 0usize;

    for case in &cases {
        let source = fs::read_to_string(case)
            .map_err(|error| format!("failed to read {}: {error}", case.display()))?;
        let file_name = case.to_string_lossy().into_owned();

        let ts_rust_side = ts_rust_diagnostics(&checker, &source, &file_name)?;
        let tsc_side = tsc_diagnostics(&tsc_bin, case)?;

        let relative = case.strip_prefix(root).unwrap_or(case);
        if ts_rust_side.positions == tsc_side.positions {
            println!("ok   {}", relative.display());
        } else {
            mismatches += 1;
            println!("FAIL {}", relative.display());
            print_side("ts-rust", &ts_rust_side);
            print_side("tsc", &tsc_side);
        }
    }

    if mismatches == 0 {
        println!(
            "all {} fixture(s) agree with tsc {expected_version} on error position/severity",
            cases.len()
        );
        Ok(true)
    } else {
        eprintln!(
            "{mismatches} of {} fixture(s) disagree with tsc {expected_version}",
            cases.len()
        );
        Ok(false)
    }
}

fn print_side(label: &str, side: &SideDiagnostics) {
    if side.full_lines.is_empty() {
        println!("  {label}: (no diagnostics)");
        return;
    }
    println!("  {label}:");
    for line in &side.full_lines {
        println!("    {line}");
    }
}

struct Options {
    tsc_bin: String,
    case: Option<String>,
}

fn parse_options() -> Result<Option<Options>, String> {
    let mut arguments = env::args_os();
    let _program = arguments.next();
    let mut tsc_bin = env::var(TSC_BIN_ENV).unwrap_or_else(|_| "npx --no-install tsc".to_owned());
    let mut case = None;

    while let Some(argument) = arguments.next() {
        match argument.to_str() {
            Some("--tsc") => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "--tsc requires a command".to_owned())?;
                tsc_bin = value
                    .into_string()
                    .map_err(|_| "--tsc must be valid UTF-8".to_owned())?;
            }
            Some("--case") => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "--case requires a fixture name".to_owned())?;
                case = Some(
                    value
                        .into_string()
                        .map_err(|_| "--case must be valid UTF-8".to_owned())?,
                );
            }
            Some("--help" | "-h") => {
                print_usage();
                return Ok(None);
            }
            Some(other) => return Err(format!("unknown argument '{other}'")),
            None => return Err("arguments must be valid UTF-8".to_owned()),
        }
    }

    Ok(Some(Options { tsc_bin, case }))
}

fn print_usage() {
    println!(
        "Usage: cargo run --bin compare-tsc -- [--case <fixture>] [--tsc <command>]\n\
         \n\
         Compares ts-rust's diagnostics against tsc for every fixture under\n\
         {CONFORMANCE_DIR}/, by (line, severity) only -- see the module doc\n\
         comment in bin/compare-tsc.rs for why message text is not compared.\n\
         \n\
         The installed tsc must report the exact version pinned in\n\
         {REFERENCE_FILE}; a mismatch is a hard error, not a warning.\n\
         \n\
         {TSC_BIN_ENV} may be used instead of --tsc."
    );
}

fn read_reference_version(path: &Path) -> Result<String, String> {
    let content = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let versions: Vec<_> = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    let [version] = versions.as_slice() else {
        return Err(format!(
            "{} must contain exactly one reference version line",
            path.display()
        ));
    };
    Ok((*version).to_owned())
}

fn verify_tsc_version(tsc_bin: &str, expected: &str) -> Result<(), String> {
    let output = run_shell(tsc_bin, &["--version"])
        .map_err(|error| format!("failed to run '{tsc_bin} --version': {error}"))?;
    let actual = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if actual != expected {
        return Err(format!(
            "tsc version mismatch: expected '{expected}', found '{actual}'; reference updates \
             require reviewing every case under {CONFORMANCE_DIR}/ against the new version and \
             editing {REFERENCE_FILE} deliberately, not silently"
        ));
    }
    Ok(())
}

fn discover_cases(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut cases: Vec<_> = fs::read_dir(root)
        .map_err(|error| format!("failed to read {}: {error}", root.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "ts"))
        .collect();
    cases.sort();
    Ok(cases)
}

fn case_matches(path: &Path, requested: &str) -> bool {
    path.file_name().is_some_and(|name| {
        name == requested || path.file_stem().is_some_and(|stem| stem == requested)
    })
}

fn ts_rust_diagnostics(
    checker: &TypeChecker,
    source: &str,
    file_name: &str,
) -> Result<SideDiagnostics, String> {
    let result = checker
        .check_source(source, file_name)
        .map_err(|error| format!("{file_name}: ts-rust failed to parse: {error}"))?;
    let line_index = LineIndex::new(source);

    let mut positions = Vec::with_capacity(result.diagnostics.len());
    let mut full_lines = Vec::with_capacity(result.diagnostics.len());
    for diagnostic in &result.diagnostics {
        let (line, column) = line_index.line_col_utf8(diagnostic.start, source);
        let is_error = matches!(diagnostic.severity, Severity::Error);
        full_lines.push(format!(
            "{line}:{column} {} {}",
            if is_error { "error" } else { "warning" },
            diagnostic.message
        ));
        if !is_error {
            // Warnings in ts-rust are currently all "not yet checked" markers
            // (see bridge/statements/support.rs and friends) -- implementation
            // status, not a type-checking verdict. tsc has no equivalent
            // concept, so including them here would compare against nothing
            // and report spurious disagreement on every unimplemented
            // construct. Still printed above for human visibility on FAIL.
            continue;
        }
        positions.push(ComparablePosition { line, is_error });
    }
    positions.sort();
    Ok(SideDiagnostics {
        positions,
        full_lines,
    })
}

fn tsc_diagnostics(tsc_bin: &str, case: &Path) -> Result<SideDiagnostics, String> {
    let case_arg = case.to_string_lossy().into_owned();
    let output = run_shell(
        tsc_bin,
        &[
            "--strict",
            "--noEmit",
            "--pretty",
            "false",
            "--noErrorTruncation",
            "--skipLibCheck",
            &case_arg,
        ],
    )
    .map_err(|error| format!("failed to run '{tsc_bin}' on {}: {error}", case.display()))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let text = format!("{stdout}{stderr}");

    let mut positions = Vec::new();
    let mut full_lines = Vec::new();
    for line in text.lines() {
        let Some(parsed) = parse_tsc_line(line) else {
            continue;
        };
        let (row, col, is_error, message) = parsed;
        full_lines.push(format!(
            "{row}:{col} {} {message}",
            if is_error { "error" } else { "warning" }
        ));
        if !is_error {
            continue;
        }
        positions.push(ComparablePosition {
            line: row,
            is_error,
        });
    }
    positions.sort();
    Ok(SideDiagnostics {
        positions,
        full_lines,
    })
}

// Parses one line of `tsc --pretty false` output, shaped like:
//   file.ts(12,5): error TS2322: Type 'string' is not assignable to type 'number'.
// Returns (line, column, is_error, message) on a match, without a regex
// dependency -- tsc's format is stable and simple enough for plain string ops.
fn parse_tsc_line(line: &str) -> Option<(u32, u32, bool, String)> {
    let open = line.find('(')?;
    let close = line[open..].find(')')? + open;
    let (row_str, col_str) = line[open + 1..close].split_once(',')?;
    let row: u32 = row_str.trim().parse().ok()?;
    let col: u32 = col_str.trim().parse().ok()?;

    let rest = line[close + 1..].trim_start().trim_start_matches(':').trim_start();
    let (kind, message) = if let Some(message) = rest.strip_prefix("error") {
        (true, message)
    } else if let Some(message) = rest.strip_prefix("warning") {
        (false, message)
    } else {
        return None;
    };

    Some((row, col, kind, message.trim_start().to_owned()))
}

fn run_shell(command: &str, args: &[&str]) -> std::io::Result<std::process::Output> {
    // tsc_bin may itself be a multi-word command ("npx --no-install tsc"), so
    // it is run through a shell rather than as a single argv[0].
    let joined = format!("{command} {}", args.join(" "));
    Command::new("bash").arg("-lc").arg(joined).output()
}
