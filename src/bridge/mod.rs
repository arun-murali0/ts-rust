mod context;
mod declare;
mod expressions;
mod narrow;
mod parse;
mod statements;

use oxc_allocator::Allocator;

use crate::diagnostics::Diagnostic;
use crate::error::CheckerError;
use context::CheckContext;

pub use parse::parse;

pub fn parse_and_bind_only(source: &str, file_name: &str) -> Result<(), CheckerError> {
    let allocator = Allocator::default();
    let program = parse(&allocator, source, file_name)?;
    let _scoping = parse::analyze(&program);
    Ok(())
}

// The whole checking pipeline in two passes: declare_top_level resolves every
// top-level signature first, so a function can call another declared later in the
// same file, then check_top_level walks statement and expression bodies against
// those already-resolved signatures.
#[tracing::instrument(skip_all, fields(file_name))]
pub fn check_program(source: &str, file_name: &str) -> Result<Vec<Diagnostic>, CheckerError> {
    let allocator = Allocator::default();
    let program = parse(&allocator, source, file_name)?;
    let scoping = parse::analyze(&program);

    let mut ctx = CheckContext::new(file_name);

    declare::declare_top_level(&program, &mut ctx);
    statements::check_top_level(&program, &scoping, &mut ctx);

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
        if issue.given == 0 {
            // A bare reference to a generic type stays a warning: it still
            // resolves, with its type parameters left as placeholders.
            ctx.warning(
                crate::diagnostic_messages::messages::generic_type_missing_type_arguments(
                    name,
                    issue.required,
                ),
                issue.span,
            );
        } else if issue.expected == 0 {
            ctx.error(
                crate::diagnostic_messages::messages::type_is_not_generic(name),
                issue.span,
            );
        } else {
            ctx.error(
                crate::diagnostic_messages::messages::type_argument_count_mismatch(
                    name,
                    issue.required,
                    issue.expected,
                    issue.given,
                ),
                issue.span,
            );
        }
    }

    tracing::info!(diagnostic_count = ctx.diagnostics.len(), "check complete");
    Ok(ctx.diagnostics)
}
