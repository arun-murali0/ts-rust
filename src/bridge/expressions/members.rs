use oxc_ast::ast::Expression;
use oxc_semantic::Scoping;
use oxc_span::{GetSpan, Span};

use crate::arena::{TypeArena, TypeId};
use crate::types::Type;

use super::super::context::CheckContext;
use super::super::narrow::narrow_to_non_nullish;
use super::infer_expression_type;

// Member access is modelled on Type::Object, on a union of types that all have the
// property, and on `.length` for strings and arrays. `.length` is the only
// String/Array member, deliberately: every other String.prototype and
// Array.prototype method (`.map`, `.slice`, `.indexOf`, ...) would need a whole
// method signature modelled, and the array ones take a callback whose parameter
// types have to be inferred from the element type, which is a feature in its own
// right. `.length` is a fixed property returning `number` on both, so it needs
// neither.
//
// A union is looked through, not rejected: as in tsc, a property is readable on a
// union when every member has it, and its type is the union of the members'
// property types. Without this, `shape.kind` on an un-narrowed discriminated union
// would be reported as missing, so the very read that a `kind` check narrows on
// would be a false error before narrowing ever ran. A member that lacks the
// property makes the whole access an error, reported once on the union rather than
// once per member.
pub(crate) fn infer_member_access_type(
    object_type: TypeId,
    property_name: &str,
    span: Span,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    // A constrained generic parameter (`T extends HasLength`) exposes its
    // constraint's members inside the function body, the same way tsc treats
    // `T`'s accessible shape as its constraint's shape. An unconstrained `T`
    // has no known members, so the lookup finds nothing and reports it.
    let effective_type = match ctx.arena.get(object_type) {
        Type::GenericParameter(_, _, Some(constraint)) => *constraint,
        _ => object_type,
    };

    match lookup_member(&mut ctx.arena, effective_type, property_name) {
        Some(found) => found,
        None => {
            ctx.error(
                crate::diagnostic_messages::messages::property_does_not_exist(
                    &ctx.arena,
                    property_name,
                    effective_type,
                ),
                span,
            );
            ctx.arena.error()
        }
    }
}

// The type of `property_name` on `type_id`, or None when it is not there. It reports
// nothing itself, so a union can ask every member and let the caller report once.
// `any` and the error type answer with the error type: they are compatible with
// everything, which stops one failure from cascading into more diagnostics.
fn lookup_member(arena: &mut TypeArena, type_id: TypeId, property_name: &str) -> Option<TypeId> {
    let effective = match arena.get(type_id) {
        Type::GenericParameter(_, _, Some(constraint)) => *constraint,
        _ => type_id,
    };

    match arena.get(effective) {
        Type::Any | Type::Error => Some(arena.error()),
        Type::String | Type::StringLiteral(_) | Type::Array(_) if property_name == "length" => {
            Some(arena.number())
        }
        Type::Object(object) => {
            match object.properties.iter().find(|p| *p.name == *property_name) {
                Some(property) => Some(property.type_id),
                None => arena.record_value_type(effective),
            }
        }
        Type::Union(members) => {
            let members = members.clone();
            let mut found = Vec::with_capacity(members.len());
            for member in members {
                found.push(lookup_member(arena, member, property_name)?);
            }
            Some(arena.alloc_union(found))
        }
        _ => None,
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
    let key_type = infer_expression_type(key_expr, scoping, ctx);

    // A Record<K, V>-tagged object accepts any key and yields V -- see
    // TypeArena::record_value_type's own doc comment on why K itself is not
    // checked here.
    if let Some(value_type) = ctx.arena.record_value_type(object_type) {
        return value_type;
    }

    if let &Type::Array(element_type) = ctx.arena.get(object_type) {
        // Every access to an array element, whether the index is a literal or
        // not, includes undefined: a literal index being in range is no more
        // provable at compile time than a variable one is, since this checker
        // (like tsc under noUncheckedIndexedAccess) does not track array
        // lengths. `arr[0]` and `arr[i]` are both indexing past the end of a
        // real array at runtime if the array turns out to be empty.
        return ctx
            .arena
            .alloc_union(vec![element_type, ctx.arena.undefined()]);
    }

    let Expression::StringLiteral(key) = key_expr else {
        // A key typed plain `string`, `number` or `any` cannot name any particular
        // property, and a plain object type has no index signature to fall back
        // on, so tsc rejects the access under noImplicitAny. Any other key (a
        // literal type, a union of literals, a generic parameter) may well be
        // valid, and is a shape this checker cannot evaluate yet, so it is still
        // reported as unsupported rather than guessed at.
        let plain_object = matches!(ctx.arena.get(object_type), Type::Object(_));
        let plain_key = matches!(
            ctx.arena.get(key_type),
            Type::String | Type::Number | Type::Any
        );
        if plain_object && plain_key {
            ctx.error(
                crate::diagnostic_messages::messages::element_implicitly_any(
                    &ctx.arena,
                    key_type,
                    object_type,
                ),
                span,
            );
        } else {
            ctx.warning(
                crate::diagnostic_messages::messages::unimplemented_computed_member_key(),
                span,
            );
        }
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
