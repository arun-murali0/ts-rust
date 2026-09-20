use oxc_ast::ast::Expression;
use oxc_semantic::Scoping;
use oxc_span::{GetSpan, Span};

use crate::arena::TypeId;
use crate::types::Type;

use super::super::context::CheckContext;
use super::super::narrow::narrow_to_non_nullish;
use super::infer_expression_type;

// Member access is only understood on Type::Object today; a union type (for
// example, a discriminated union not yet narrowed by its tag) is not looked
// through here, so accessing a property on an un-narrowed union reports it as
// missing even if every member happens to share that property.
pub(crate) fn infer_member_access_type(
    object_type: TypeId,
    property_name: &str,
    span: Span,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let Type::Object(object) = ctx.arena.get(object_type) else {
        if !matches!(ctx.arena.get(object_type), Type::Any | Type::Error) {
            ctx.error(
                crate::diagnostic_messages::messages::property_does_not_exist(property_name),
                span,
            );
        }
        return ctx.arena.error();
    };

    match object.properties.iter().find(|p| *p.name == *property_name) {
        Some(property) => property.type_id,
        None => {
            ctx.error(
                crate::diagnostic_messages::messages::property_does_not_exist(property_name),
                span,
            );
            ctx.arena.error()
        }
    }
}

pub(super) fn infer_member_access_type_with_optional(
    object_type: TypeId,
    property_name: &str,
    optional: bool,
    span: Span,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    if !optional {
        return infer_member_access_type(object_type, property_name, span, ctx);
    }

    // a?.b short-circuits to undefined at runtime when a is null or undefined, so
    // the property is looked up on the non-nullish narrowing of the object type,
    // and undefined is added back into the result to reflect that short-circuit.
    let non_nullish = narrow_to_non_nullish(&mut ctx.arena, object_type);
    let property_type = infer_member_access_type(non_nullish, property_name, span, ctx);
    ctx.arena
        .alloc_union(vec![property_type, ctx.arena.undefined()])
}

pub(super) fn infer_computed_member_access_type(
    object_type: TypeId,
    key_expr: &Expression,
    optional: bool,
    span: Span,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    infer_expression_type(key_expr, scoping, ctx);

    if let &Type::Array(element_type) = ctx.arena.get(object_type) {
        return match key_expr {
            // arr[0] with a literal index is assumed in bounds and yields the
            // element type directly. Any other key, a variable index for
            // example, cannot be proven in bounds, so undefined is included to
            // reflect that indexing past the end of a real array yields
            // undefined at runtime.
            Expression::NumericLiteral(_) => element_type,
            _ => ctx
                .arena
                .alloc_union(vec![element_type, ctx.arena.undefined()]),
        };
    }

    let Expression::StringLiteral(key) = key_expr else {
        ctx.warning(
            crate::diagnostic_messages::messages::unimplemented_computed_member_key(),
            span,
        );
        return ctx.arena.error();
    };

    infer_member_access_type_with_optional(object_type, &key.value, optional, span, ctx)
}

pub(super) fn infer_chain_element_type(
    element: &oxc_ast::ast::ChainElement,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    use oxc_ast::ast::ChainElement;

    match element {
        ChainElement::StaticMemberExpression(member) => {
            let object_type = infer_expression_type(&member.object, scoping, ctx);
            infer_member_access_type_with_optional(
                object_type,
                &member.property.name,
                member.optional,
                member.span(),
                ctx,
            )
        }
        ChainElement::ComputedMemberExpression(member) => {
            let object_type = infer_expression_type(&member.object, scoping, ctx);
            infer_computed_member_access_type(
                object_type,
                &member.expression,
                member.optional,
                member.span(),
                scoping,
                ctx,
            )
        }
        _ => {
            ctx.warning(
                crate::diagnostic_messages::messages::unimplemented_optional_chain_link(),
                element.span(),
            );
            ctx.arena.error()
        }
    }
}
