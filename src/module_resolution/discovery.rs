use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;

// One import or re-export statement's request for another module, as the parser
// recorded it. `is_import` is false for `export ... from`, which depends on the other
// module just as much but binds nothing in this one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleRequest {
    pub specifier: String,
    pub is_type: bool,
    pub is_import: bool,
    pub start: u32,
}

// What a file asks for, and whether the parse behind it can be trusted. A recoverable
// syntax error leaves the requests intact, since the parser still recorded every
// statement it got through; only an unrecoverable one empties them. In both cases
// `parse_failed` is set, so a file that lost edges is never mistaken for one that has
// none.
#[derive(Debug, Default)]
pub struct ModuleScan {
    pub requests: Vec<ModuleRequest>,
    pub parse_failed: bool,
}

pub fn scan_module_requests(source: &str, file_name: &str) -> ModuleScan {
    let allocator = Allocator::default();
    let source_type = SourceType::from_path(file_name)
        .unwrap_or_else(|_| SourceType::default().with_typescript(true));
    let parsed = Parser::new(&allocator, source, source_type).parse();
    let parse_failed = parsed.diagnostics.has_errors();

    if parsed.fatal_error {
        return ModuleScan {
            requests: Vec::new(),
            parse_failed,
        };
    }

    let mut requests = Vec::new();
    for (specifier, entries) in parsed.module_record.requested_modules.iter() {
        for entry in entries.iter() {
            requests.push(ModuleRequest {
                specifier: specifier.as_str().to_owned(),
                is_type: entry.is_type,
                is_import: entry.is_import,
                start: entry.span.start,
            });
        }
    }
    // The parser keeps requests in a hash map keyed by specifier, whose iteration order
    // changes from run to run. Source order makes the graph reproducible.
    requests.sort_by_key(|request| request.start);

    ModuleScan {
        requests,
        parse_failed,
    }
}

#[cfg(test)]
mod tests {
    use super::scan_module_requests;

    #[test]
    fn requests_come_back_in_source_order() {
        let scan = scan_module_requests(
            "import { b } from './b';\nimport type { C } from './c';\nexport * from './a';",
            "main.ts",
        );
        let specifiers: Vec<_> = scan
            .requests
            .iter()
            .map(|request| request.specifier.as_str())
            .collect();
        assert_eq!(specifiers, ["./b", "./c", "./a"]);
        assert!(!scan.parse_failed);
    }

    #[test]
    fn type_only_and_re_export_requests_are_told_apart() {
        let scan = scan_module_requests(
            "import type { T } from './t';\nexport { x } from './x';\nimport { v } from './v';",
            "main.ts",
        );
        let flags: Vec<_> = scan
            .requests
            .iter()
            .map(|request| (request.is_type, request.is_import))
            .collect();
        assert_eq!(flags, [(true, true), (false, false), (false, true)]);
    }

    #[test]
    fn a_syntax_error_is_reported_and_not_mistaken_for_no_imports() {
        let scan = scan_module_requests("import { a } from './a'; const = ;", "broken.ts");
        assert!(scan.parse_failed);
    }
}
