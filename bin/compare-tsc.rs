//! Compares ts-rust's own diagnostics against a real `tsc` run, in-process.
//!
//! Unlike scripts/harness.sh and scripts/compare-local.sh, this binary never
//! shells out to ts-rust as a subprocess -- it calls `TypeChecker::check_source`
//! directly, the same entry point bin/ts-rust.rs uses, and only shells out to
//! `tsc` itself (there is no Rust API for that side of the comparison).
//!
//! Pass/fail is decided by line number alone, errors only -- severity is a
//! filter applied before comparison (only Severity::Error diagnostics ever
//! become an Entry on either side), not a field the comparison itself
//! checks. ts-rust's own code (TSR####, see diagnostic_codes.rs) is its own
//! namespace, not tsc's TS#### numbering, and message wording is
//! independently written -- so comparing message text or code text
//! verbatim against tsc's own would report near-constant false mismatches
//! even on a real agreement. Both codes are still shown side by side in the
//! table, as context for a human, just not used for the pass/fail check
//! itself. Warnings are excluded entirely, not just from the check: every
//! warning ts-rust currently emits is a "not yet checked" implementation-
//! status marker (see bridge/statements/support.rs), which has no tsc
//! equivalent at all, so a warning never becomes an Entry on the ts-rust
//! side and is filtered out of tsc's own output the same way.
//!
//! Fixtures live under tests/tsc-conformance/, a directory dedicated to this
//! comparison. tests/fixtures/ is NOT used here: several of those fixtures
//! deliberately exercise this checker's own recovery behavior (for example,
//! a class with one unresolvable member being treated as wholly unsupported)
//! and are expected to diverge from tsc, so mixing them in would produce
//! constant, meaningless failures.
//!
//! IMPORTANT: this binary cannot yet point at a real multi-file project
//! (e.g. zustand). ts-rust has no cross-file import/export resolution yet,
//! so every file in a multi-file project would report spurious "cannot see
//! this import" noise -- see the module resolution design work for why.
//! Single-file conformance fixtures are the only meaningful input today.

use std::{
    collections::BTreeMap,
    env, fs,
    io::IsTerminal,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

use ts_rust::{LineIndex, Severity, TypeChecker};

const REFERENCE_FILE: &str = "tests/tsc-reference.txt";
const CONFORMANCE_DIR: &str = "tests/tsc-conformance";
const TSC_BIN_ENV: &str = "TSC_BIN";
const NO_COLOR_ENV: &str = "NO_COLOR";

// --- minimal hand-rolled ANSI styling, no crate for this one --------------

struct Style {
    enabled: bool,
}

impl Style {
    fn detect() -> Self {
        let enabled =
            env::var_os(NO_COLOR_ENV).is_none() && std::io::stdout().is_terminal();
        Self { enabled }
    }

    fn paint(&self, code: &str, text: &str) -> String {
        if self.enabled {
            format!("\x1b[{code}m{text}\x1b[0m")
        } else {
            text.to_owned()
        }
    }

    fn bold(&self, text: &str) -> String {
        self.paint("1", text)
    }
    fn dim(&self, text: &str) -> String {
        self.paint("2", text)
    }
    fn green(&self, text: &str) -> String {
        self.paint("32", text)
    }
    fn red(&self, text: &str) -> String {
        self.paint("31", text)
    }
    fn yellow(&self, text: &str) -> String {
        self.paint("33", text)
    }
    fn cyan(&self, text: &str) -> String {
        self.paint("36", text)
    }
}

// --- diagnostic model -------------------------------------------------------

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ComparablePosition {
    line: u32,
}

#[derive(Clone)]
struct Entry {
    line: u32,
    column: u32,
    code: Option<String>,
    message: String,
}

struct SideDiagnostics {
    positions: Vec<ComparablePosition>,
    entries: Vec<Entry>,
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

    let style = Style::detect();
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
    let mut passed = 0usize;
    let mut failed = 0usize;

    println!(
        "{}",
        style.dim(&format!(
            "comparing against tsc {expected_version}  ({} fixture{})",
            cases.len(),
            if cases.len() == 1 { "" } else { "s" }
        ))
    );
    println!();

    for case in &cases {
        let source = fs::read_to_string(case)
            .map_err(|error| format!("failed to read {}: {error}", case.display()))?;
        let file_name = case.to_string_lossy().into_owned();

        let ts_rust_side = ts_rust_diagnostics(&checker, &source, &file_name)?;
        let tsc_side = tsc_diagnostics(&tsc_bin, case)?;
        let relative = case.strip_prefix(root).unwrap_or(case);
        let ok = ts_rust_side.positions == tsc_side.positions;

        if ok {
            passed += 1;
        } else {
            failed += 1;
        }

        print_case(&style, relative, ok, &ts_rust_side, &tsc_side);
        println!();
    }

    print_summary(&style, passed, failed, &expected_version);

    Ok(failed == 0)
}

// --- rendering ---------------------------------------------------------

const LINE_COL_WIDTH: usize = 6;
const MSG_COL_WIDTH: usize = 42;

fn box_line(left: char, mid: char, right: char) -> String {
    let segment = |width: usize| "─".repeat(width + 2);
    format!(
        "{left}{}{mid}{}{mid}{}{right}",
        segment(LINE_COL_WIDTH),
        segment(MSG_COL_WIDTH),
        segment(MSG_COL_WIDTH),
    )
}

fn print_case(
    style: &Style,
    relative: &Path,
    ok: bool,
    ts_rust_side: &SideDiagnostics,
    tsc_side: &SideDiagnostics,
) {
    let title = relative.display().to_string();
    let verdict = if ok {
        style.green("MATCH")
    } else {
        style.red("DIFFER")
    };

    println!("  {}  {}", style.bold(&title), verdict);
    println!("  {}", style.dim(&box_line('┌', '┬', '┐')));
    println!(
        "  │ {:<lw$} │ {:<mw$} │ {:<mw$} │",
        "line",
        "ts-rust",
        "tsc",
        lw = LINE_COL_WIDTH,
        mw = MSG_COL_WIDTH,
    );
    println!("  {}", style.dim(&box_line('├', '┼', '┤')));

    let mut by_line: BTreeMap<u32, (Vec<&Entry>, Vec<&Entry>)> = BTreeMap::new();
    for entry in &ts_rust_side.entries {
        by_line.entry(entry.line).or_default().0.push(entry);
    }
    for entry in &tsc_side.entries {
        by_line.entry(entry.line).or_default().1.push(entry);
    }

    for (group_index, (line, (ts_rust_entries, tsc_entries))) in by_line.iter().enumerate() {
        if group_index > 0 {
            println!("  {}", style.dim(&box_line('├', '┼', '┤')));
        }

        let both_present = !ts_rust_entries.is_empty() && !tsc_entries.is_empty();
        let rows = ts_rust_entries.len().max(tsc_entries.len()).max(1);

        for row in 0..rows {
            let left = ts_rust_entries
                .get(row)
                .map(|e| {
                    let with_code = match &e.code {
                        Some(code) if !code.is_empty() => format!("{code} {}", e.message),
                        _ => e.message.clone(),
                    };
                    truncate(&with_code, MSG_COL_WIDTH)
                })
                .unwrap_or_else(|| style.dim("—"));
            let right = tsc_entries
                .get(row)
                .map(|e| {
                    let with_code = match &e.code {
                        Some(code) if !code.is_empty() => format!("{code} {}", e.message),
                        _ => e.message.clone(),
                    };
                    truncate(&with_code, MSG_COL_WIDTH)
                })
                .unwrap_or_else(|| style.dim("—"));

            let line_label = if row == 0 {
                format!("{line}")
            } else {
                String::new()
            };
            let colored_line = if both_present {
                style.green(&line_label)
            } else if !line_label.is_empty() {
                style.yellow(&line_label)
            } else {
                line_label
            };

            println!(
                "  │ {:<lw$} │ {:<mw$} │ {:<mw$} │",
                colored_line,
                left,
                right,
                lw = LINE_COL_WIDTH,
                mw = MSG_COL_WIDTH,
            );
        }

        if !both_present {
            let diagnosis = if tsc_entries.is_empty() {
                "ts-rust flagged an error tsc did not -- likely a false positive, or a \
                 check that's stricter than real TS semantics here"
            } else {
                "tsc flagged an error ts-rust did not -- likely a detection gap: this \
                 construct may not be checked yet"
            };
            // Spans the two message columns as one wide cell -- the merged
            // width matches MSG_COL_WIDTH*2+3 (each column plus the " │ "
            // divider between them), so the right border still lines up
            // even though there's no internal divider for this one row.
            let merged_width = MSG_COL_WIDTH * 2 + 3;
            let text = truncate(&format!("↳ {diagnosis}"), merged_width);
            println!(
                "  │ {:<lw$} │ {} │",
                "",
                style.cyan(&format!("{text:<merged_width$}")),
                lw = LINE_COL_WIDTH,
            );
        }
    }

    println!("  {}", style.dim(&box_line('└', '┴', '┘')));
}

fn print_summary(style: &Style, passed: usize, failed: usize, expected_version: &str) {
    let total = passed + failed;
    if failed == 0 {
        println!(
            "{} all {total} fixture(s) agree with tsc {expected_version} on error position/severity",
            style.green("✓")
        );
    } else {
        println!(
            "{} {failed} of {total} fixture(s) disagree with tsc {expected_version} ({passed} agree)",
            style.red("✗")
        );
    }
}

fn truncate(text: &str, width: usize) -> String {
    let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= width {
        collapsed
    } else {
        let mut out: String = collapsed.chars().take(width.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

// --- options / setup -----------------------------------------------------

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
         {CONFORMANCE_DIR}/, by (line, severity) only, errors only -- see the\n\
         module doc comment in bin/compare-tsc.rs for why.\n\
         \n\
         The installed tsc must report the exact version pinned in\n\
         {REFERENCE_FILE}; a mismatch is a hard error, not a warning.\n\
         \n\
         {TSC_BIN_ENV} may be used instead of --tsc.\n\
         {NO_COLOR_ENV}=1 disables colored output; color is also auto-disabled\n\
         when stdout is not a terminal (e.g. piped to a file or CI log)."
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

// --- collecting each side's diagnostics -----------------------------------

fn ts_rust_diagnostics(
    checker: &TypeChecker,
    source: &str,
    file_name: &str,
) -> Result<SideDiagnostics, String> {
    let result = checker
        .check_source(source, file_name)
        .map_err(|error| format!("{file_name}: ts-rust failed to parse: {error}"))?;
    let line_index = LineIndex::new(source);

    let mut positions = Vec::new();
    let mut entries = Vec::new();
    for diagnostic in &result.diagnostics {
        if !matches!(diagnostic.severity, Severity::Error) {
            // Warnings are all "not yet checked" implementation-status
            // markers with no tsc equivalent -- see the module doc comment.
            continue;
        }
        let (line, column) = line_index.line_col_utf8(diagnostic.start, source);
        positions.push(ComparablePosition { line });
        entries.push(Entry {
            line,
            column,
            code: Some(diagnostic.code.as_str().to_owned()),
            message: diagnostic.message.clone(),
        });
    }
    positions.sort();
    entries.sort_by_key(|entry| entry.line);
    Ok(SideDiagnostics { positions, entries })
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
    let mut entries = Vec::new();
    for line in text.lines() {
        let Some((row, col, is_error, code, message)) = parse_tsc_line(line) else {
            continue;
        };
        if !is_error {
            continue;
        }
        positions.push(ComparablePosition { line: row });
        entries.push(Entry {
            line: row,
            column: col,
            code: Some(code),
            message,
        });
    }
    positions.sort();
    entries.sort_by_key(|entry| entry.line);
    Ok(SideDiagnostics { positions, entries })
}

// Parses one line of `tsc --pretty false` output, shaped like:
//   file.ts(12,5): error TS2322: Type 'string' is not assignable to type 'number'.
// Returns (line, column, is_error, code, message) on a match, without a
// regex dependency -- tsc's format is stable and simple enough for plain
// string ops.
fn parse_tsc_line(line: &str) -> Option<(u32, u32, bool, String, String)> {
    let open = line.find('(')?;
    let close = line[open..].find(')')? + open;
    let (row_str, col_str) = line[open + 1..close].split_once(',')?;
    let row: u32 = row_str.trim().parse().ok()?;
    let col: u32 = col_str.trim().parse().ok()?;

    let rest = line[close + 1..]
        .trim_start()
        .trim_start_matches(':')
        .trim_start();
    let (is_error, after_kind) = if let Some(rest) = rest.strip_prefix("error") {
        (true, rest)
    } else {
        let rest = rest.strip_prefix("warning")?;
        (false, rest)
    };

    let after_kind = after_kind.trim_start();
    let (code, message) = match after_kind.split_once(':') {
        Some((code, message)) if code.starts_with("TS") => {
            (code.trim().to_owned(), message.trim_start().to_owned())
        }
        _ => (String::new(), after_kind.to_owned()),
    };

    Some((row, col, is_error, code, message))
}

fn run_shell(command: &str, args: &[&str]) -> std::io::Result<std::process::Output> {
    // tsc_bin may itself be a multi-word command ("npx --no-install tsc"), so
    // it is run through a shell rather than as a single argv[0].
    let joined = format!("{command} {}", args.join(" "));
    Command::new("bash").arg("-lc").arg(joined).output()
}
