use oxc_ast::ast::Function;
use oxc_semantic::Scoping;

use super::super::context::CheckContext;
use super::support::report_implicit_any_params;
use super::{bind_params, check_statement};

pub(super) fn check_function_declaration(
    func: &Function,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) {
    report_implicit_any_params(&func.params, ctx);

    let Some(body) = &func.body else { return };
    let Some(name) = func.id.as_ref() else { return };
    let Some(symbol_id) = name.symbol_id.get() else {
        return;
    };

    // Reuses the signature declare_top_level already resolved, rather than
    // re-resolving parameter and return types here, so the body is checked
    // against exactly the same types the function was declared with.
    let Some(function_type) = ctx.symbols.get(symbol_id) else {
        tracing::trace!(name = %name.name, "function signature not fully annotated, body not checked");
        return;
    };
    let crate::types::Type::Function(function_type) = ctx.arena.get(function_type).clone() else {
        return;
    };

    bind_params(&func.params, &function_type.params, scoping, ctx);

    // Pushed again here, separately from declare_top_level's own push, since the
    // declare pass and this body-check pass are two independent traversals. Both
    // must resolve func's own T to the exact same GenericParameter node, which is
    // what TypeNamespace's per-parameter identity cache guarantees; without it, a
    // T[] annotation written inside the body would not match the parameter T
    // came from.
    let type_param_scope = ctx.namespace.push_type_params(&mut ctx.arena, func);

    let outer_return_type = ctx.current_return_type.replace(function_type.return_type);
    for body_stmt in &body.statements {
        check_statement(body_stmt, scoping, ctx);
    }
    ctx.current_return_type = outer_return_type;

    ctx.namespace.pop_type_params(type_param_scope);
}
