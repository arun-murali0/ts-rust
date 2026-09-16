use oxc_ast::ast::IdentifierReference;
use oxc_semantic::Scoping;
use oxc_span::GetSpan;

use crate::arena::TypeId;

use super::super::context::CheckContext;
use super::super::narrow::resolve_symbol_id;

// The narrow overlay is checked before the symbol's own declared type, since a
// variable narrowed in the current branch, for example by an earlier if
// (typeof x === "string"), should read as that narrowed type here, not the wider
// type it was originally declared with.
pub(crate) fn resolve_identifier_type(
    ident: &IdentifierReference,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let Some(symbol_id) = resolve_symbol_id(ident, scoping) else {
        ctx.error(format!("Cannot find name '{}'.", ident.name), ident.span());
        return ctx.arena.error();
    };

    if let Some(narrowed) = ctx.narrow.get(symbol_id) {
        return narrowed;
    }

    match ctx.symbols.get(symbol_id) {
        Some(type_id) => type_id,
        // oxc's own scope analysis resolved this identifier to a real binding, so
        // this only happens if declare_top_level left that binding's type
        // unregistered, an unresolvable annotation, for instance. Warned rather
        // than silently defaulted, since it points at a gap in this checker
        // rather than a mistake in the source being checked.
        None => {
            tracing::warn!(
                name = %ident.name,
                "resolved symbol has no registered type, falling back to the error sentinel"
            );
            ctx.arena.error()
        }
    }
}
