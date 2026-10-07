mod context;
mod declare;
mod expressions;
mod narrow;
mod parse;
mod statements;
mod unreachable_code;

use oxc_allocator::Allocator;

use crate::arena::{TypeArena, TypeArenaStats};
use crate::diagnostics::Diagnostic;
use crate::error::CheckerError;
use crate::namespace::NamespaceStats;
use crate::semantic::queries::QueryStats;
use crate::types::FileId;
use context::CheckContext;

pub use parse::parse;

pub fn parse_and_bind_only(source: &str, file_name: &str) -> Result<(), CheckerError> {
    let allocator = Allocator::default();
    let program = parse(&allocator, source, file_name)?;
    let _semantic = parse::analyze(&program);
    Ok(())
}

/// Sizes and cache counters from one check, for benchmarks and regression reports.
pub struct CheckMetrics {
    pub arena: TypeArenaStats,
    pub queries: QueryStats,
    pub namespace: NamespaceStats,
}

// The whole checking pipeline in two passes: declare_top_level resolves every
// top-level signature first, so a function can call another declared later in the
// same file, then check_top_level walks statement and expression bodies against
// those already-resolved signatures.
//
// The arena is borrowed, not created here, so a CheckSession can keep one allocation
// across successive versions of a file. It is cleared first, so nothing from the
// previous check (types, names, intern entries) is visible to this one, and handed
// back at the end with the retained capacity.
pub fn check_program_with_state(
    source: &str,
    file_name: &str,
    file_id: FileId,
    arena: &mut TypeArena,
) -> Result<(Vec<Diagnostic>, CheckMetrics), CheckerError> {
    arena.clear();

    let allocator = Allocator::default();
    let program = parse(&allocator, source, file_name)?;
    // semantic is kept for the whole check on purpose: it owns the Scoping borrowed
    // here, and it is where the control flow graph lives once flow analysis reads
    // it. Keeping only the Scoping would drop the graph along with the rest.
    let semantic = parse::analyze(&program);
    let scoping = semantic.scoping();

    let reusable_arena = std::mem::take(arena);
    let mut ctx = CheckContext::with_arena_and_file_id(file_name, file_id, reusable_arena);

    declare::declare_top_level(&program, &mut ctx);
    statements::check_top_level(&program, scoping, &mut ctx);
    unreachable_code::check_unreachable_code(&program, &semantic, &mut ctx);

    for (name, span) in ctx.namespace.take_implicit_any_params() {
        ctx.error(
            crate::diagnostic_messages::messages::parameter_implicitly_any(&name),
            span,
        );
    }

    for (name, span) in ctx.namespace.take_unresolved_constraints() {
        ctx.warning(
            crate::diagnostic_messages::messages::unresolvable_type_parameter_constraint(&name),
            span,
        );
    }

    for issue in ctx.namespace.take_type_argument_issues() {
        let name = issue.name.as_str();
        if issue.expected == 0 {
            ctx.error(
                crate::diagnostic_messages::messages::type_is_not_generic(name),
                issue.span,
            );
        } else {
            // Includes a bare `Box` for `Box<T>` (given == 0), which tsc also
            // reports as a missing type argument.
            // tsc prints the generic with its parameter list: `Box<T>`, `Pair<A, B>`.
            let display_name = match ctx.namespace.declared_type_param_decl(name) {
                Some(decl) => {
                    let params: Vec<&str> =
                        decl.params.iter().map(|p| p.name.name.as_str()).collect();
                    format!("{name}<{}>", params.join(", "))
                }
                None => name.to_string(),
            };
            ctx.error(
                crate::diagnostic_messages::messages::type_argument_count_mismatch(
                    &display_name,
                    issue.required,
                    issue.expected,
                ),
                issue.span,
            );
        }
    }

    for (_, actual, constraint, span) in ctx.namespace.take_constraint_violations() {
        ctx.error(
            crate::diagnostic_messages::messages::type_argument_constraint_violation(
                &ctx.arena, actual, constraint,
            ),
            span,
        );
    }

    let metrics = CheckMetrics {
        arena: ctx.arena.stats(),
        queries: ctx.relation_cache.stats(),
        namespace: ctx.namespace.stats(),
    };
    tracing::info!(
        diagnostic_count = ctx.diagnostics.len(),
        arena_types = metrics.arena.type_count,
        relation_hits = metrics.queries.relation_hits,
        relation_misses = metrics.queries.relation_misses,
        generic_instantiations = metrics.namespace.instantiations,
        "check complete"
    );

    // tsc reports in source order. Checking is not in source order (declarations are
    // resolved ahead of bodies, and a call reports its own error before its arguments'),
    // so the list is put in order here. The sort is stable: two diagnostics at one
    // position keep the order they were found in.
    let mut diagnostics = std::mem::take(&mut ctx.diagnostics);
    diagnostics.sort_by_key(|diagnostic| diagnostic.start);
    *arena = ctx.arena;
    Ok((diagnostics, metrics))
}

// The one-shot entry point, for callers that check a file once and do not need to
// keep state: it owns a throwaway arena, so no FileId or arena handling leaks into
// their code.
#[tracing::instrument(skip_all, fields(file_name))]
pub fn check_program(source: &str, file_name: &str) -> Result<Vec<Diagnostic>, CheckerError> {
    let mut arena = TypeArena::new();
    Ok(check_program_with_state(source, file_name, FileId::ROOT, &mut arena)?.0)
}
