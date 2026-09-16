use oxc_ast::ast::Statement;
use oxc_span::GetSpan;

use super::super::context::CheckContext;

pub(super) fn push_unsupported(stmt: &Statement, ctx: &mut CheckContext<'_, '_>) {
    let kind = stmt_kind_name(stmt);
    tracing::trace!(kind, "unsupported statement kind");
    ctx.warning(
        format!("This statement kind is not yet checked by ts-rust: {kind}."),
        stmt.span(),
    );
}

fn stmt_kind_name(stmt: &Statement) -> &'static str {
    match stmt {
        Statement::ImportDeclaration(_) => "ImportDeclaration",
        _ => "Other",
    }
}

// Used by control_flow.rs to decide whether an if branch's narrowing should
// survive past the whole if statement. `if (x === null) return; ... x.prop`
// depends on this: since the consequent branch always exits, only the else
// branch's narrowing (x is non-null) can reach code after the if, and that
// narrowing should carry forward rather than be discarded at the closing brace.
pub(crate) fn statement_always_exits(stmt: &Statement) -> bool {
    match stmt {
        Statement::ReturnStatement(_) | Statement::ThrowStatement(_) => true,
        Statement::BlockStatement(block) => block.body.last().is_some_and(statement_always_exits),
        Statement::IfStatement(if_stmt) => match &if_stmt.alternate {
            Some(alternate) => {
                statement_always_exits(&if_stmt.consequent) && statement_always_exits(alternate)
            }
            None => false,
        },
        _ => false,
    }
}
