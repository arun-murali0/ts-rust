use oxc_ast::ast::{ObjectPropertyKind, PropertyKey};
use oxc_semantic::Scoping;

use crate::arena::TypeId;
use crate::types::{ObjectType, PropertyEntry, Type};

use super::super::context::CheckContext;
use super::infer_expression_type;

// A spread property or a computed key is silently skipped rather than making the
// whole object literal unresolvable. This is the opposite tradeoff from an
// interface or class declaration (see namespace.rs), where one unsupported
// member makes the whole declaration unresolvable: a value expression's inferred
// type is used immediately at its own use site, so under-describing it here is
// safer than the alternative of refusing to type the whole expression.
pub(super) fn infer_object_expression_type(
    object: &oxc_ast::ast::ObjectExpression,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let mut properties = Vec::with_capacity(object.properties.len());

    for property in &object.properties {
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            continue;
        };
        let PropertyKey::StaticIdentifier(key) = &property.key else {
            continue;
        };
        let type_id = infer_expression_type(&property.value, scoping, ctx);
        let type_id = crate::types::widen(&ctx.arena, type_id);
        properties.push(PropertyEntry {
            name: key.name.to_string().into(),
            type_id,
            optional: false,
        });
    }

    ctx.arena.alloc(Type::Object(ObjectType::new(properties)))
}

// Each element's type is widened (a literal 5 becomes number) before being added
// to the element-type union, matching how [1, 2, 3] should infer as number[], not
// a union of three numeric literal types. An empty array literal has nothing to
// infer an element type from, so it defaults to unknown[] rather than guessing.
pub(super) fn infer_array_expression_type(
    array: &oxc_ast::ast::ArrayExpression,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let mut element_types = Vec::with_capacity(array.elements.len());

    for element in &array.elements {
        let Some(expr) = element.as_expression() else {
            continue;
        };
        let type_id = infer_expression_type(expr, scoping, ctx);

        let type_id = crate::types::widen(&ctx.arena, type_id);
        if !element_types.contains(&type_id) {
            element_types.push(type_id);
        }
    }

    let element_type = match element_types.len() {
        0 => ctx.arena.unknown(),
        1 => element_types[0],
        _ => ctx.arena.alloc_union(element_types),
    };

    ctx.arena.alloc(Type::Array(element_type))
}
