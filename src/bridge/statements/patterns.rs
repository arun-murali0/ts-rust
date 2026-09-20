use oxc_ast::ast::{ArrayPattern, BindingPattern, ObjectPattern, PropertyKey};
use oxc_semantic::Scoping;
use oxc_span::GetSpan;

use crate::arena::TypeId;
use crate::types::{Param, Type};

use super::super::context::CheckContext;
use super::super::expressions::{infer_expression_type, infer_member_access_type};
use super::super::narrow::narrow_to_non_nullish;

pub(crate) fn bind_params(
    params: &oxc_ast::ast::FormalParameters<'_>,
    param_types: &[Param],
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) {
    for (param, declared) in params.items.iter().zip(param_types) {
        bind_pattern(&param.pattern, declared.type_id, scoping, ctx);
    }

    if let Some(rest) = &params.rest {
        if let Some(rest_param) = param_types.last().filter(|p| p.rest) {
            // oxc wraps a rest parameter's own binding one level deeper than an
            // ordinary parameter's, hence rest.rest.argument rather than
            // rest.argument, to make room for the rest parameter's own type
            // annotation alongside its binding pattern.
            bind_pattern(&rest.rest.argument, rest_param.type_id, scoping, ctx);
        }
    }
}

// Recursively binds every identifier a pattern introduces to its corresponding
// type, shared by function parameters (bind_params above) and variable
// declarators (variables.rs), since `function f({x}: T)` and `const {x} = y`
// destructure the same way once a source type is known.
pub(crate) fn bind_pattern(
    pattern: &BindingPattern,
    type_id: TypeId,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) {
    match pattern {
        BindingPattern::BindingIdentifier(id) => {
            if let Some(symbol_id) = id.symbol_id.get() {
                ctx.symbols.declare(symbol_id, type_id);
            }
        }
        BindingPattern::ObjectPattern(object) => bind_object_pattern(object, type_id, scoping, ctx),
        BindingPattern::ArrayPattern(array) => bind_array_pattern(array, type_id, scoping, ctx),
        // `{x = 1} = y` or `function f(x = 1)`: the default only ever substitutes
        // for undefined at runtime, so the effective bound type is the source
        // type with undefined (and, as an over-approximation, null too) removed,
        // unioned with the default expression's own widened type.
        BindingPattern::AssignmentPattern(assignment) => {
            let default_type = infer_expression_type(&assignment.right, scoping, ctx);
            let default_type = crate::types::widen(&ctx.arena, default_type);

            let non_nullish = narrow_to_non_nullish(&mut ctx.arena, type_id);
            let effective = ctx.arena.alloc_union(vec![non_nullish, default_type]);
            bind_pattern(&assignment.left, effective, scoping, ctx);
        }
    }
}

fn bind_object_pattern(
    object: &ObjectPattern,
    type_id: TypeId,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) {
    for property in &object.properties {
        let PropertyKey::StaticIdentifier(key) = &property.key else {
            ctx.warning(
                crate::diagnostic_messages::messages::unimplemented_destructuring_key(),
                property.span(),
            );
            continue;
        };
        let property_type = infer_member_access_type(type_id, &key.name, property.span(), ctx);
        bind_pattern(&property.value, property_type, scoping, ctx);
    }

    if let Some(rest) = &object.rest {
        // Unlike array rest below, object rest has no precise type here: the real
        // type would be the source object minus whatever properties were already
        // destructured, which this checker does not compute. Bound to Error
        // rather than left undeclared, so using the rest binding afterward does
        // not cascade into an unrelated "cannot find name" error on top of this
        // warning.
        ctx.warning(
            crate::diagnostic_messages::messages::unimplemented_rest_destructuring(),
            rest.span(),
        );
        bind_pattern(&rest.argument, ctx.arena.error(), scoping, ctx);
    }
}

fn bind_array_pattern(
    array: &ArrayPattern,
    type_id: TypeId,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) {
    let element_type = match ctx.arena.get(type_id) {
        Type::Array(element) => *element,
        Type::Any | Type::Error => type_id,
        _ => {
            ctx.error(
                crate::diagnostic_messages::messages::array_destructuring_requires_array(),
                array.span(),
            );
            ctx.arena.error()
        }
    };

    for element_pattern in array.elements.iter().flatten() {
        bind_pattern(element_pattern, element_type, scoping, ctx);
    }

    // Array rest does have a precise type, unlike object rest above: whatever is
    // left over from destructuring an array is still an array of the same
    // element type, regardless of how many elements were taken from the front.
    if let Some(rest) = &array.rest {
        let rest_type = ctx.arena.alloc(Type::Array(element_type));
        bind_pattern(&rest.argument, rest_type, scoping, ctx);
    }
}
