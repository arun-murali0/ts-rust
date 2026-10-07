use oxc_ast::ast::{ObjectPropertyKind, PropertyKey};
use oxc_semantic::Scoping;

use crate::arena::TypeId;
use crate::types::{ObjectType, PropertyEntry, Type};

use super::super::context::CheckContext;
use super::infer_expression_type;

// A computed key is silently skipped rather than making the whole object literal
// unresolvable. This is the opposite tradeoff from an interface or class
// declaration (see namespace.rs), where one unsupported member makes the whole
// declaration unresolvable: a value expression's inferred type is used immediately
// at its own use site, so under-describing it here is safer than the alternative
// of refusing to type the whole expression.
//
// A spread merges the spread object's properties in source order, so a later key
// replaces an earlier one, the same as at runtime. A spread whose type cannot be
// read as a plain object (any, an error, a union, an array, a Record) makes the
// whole literal's shape unknowable, so it is typed as the error sentinel, which is
// compatible with everything, instead of guessing a partial shape that would
// produce false "property does not exist" errors. Spreading null or undefined adds
// nothing and is fine.
pub(super) fn infer_object_expression_type(
    object: &oxc_ast::ast::ObjectExpression,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let mut properties: Vec<PropertyEntry> = Vec::with_capacity(object.properties.len());
    let mut shape_unknown = false;

    for property in &object.properties {
        let property = match property {
            ObjectPropertyKind::ObjectProperty(property) => property,
            ObjectPropertyKind::SpreadProperty(spread) => {
                let spread_type = infer_expression_type(&spread.argument, scoping, ctx);
                match ctx.arena.get(spread_type).clone() {
                    Type::Object(spread_object)
                        if ctx.arena.record_value_type(spread_type).is_none() =>
                    {
                        for entry in spread_object.properties.iter() {
                            upsert_property(&mut properties, entry.clone());
                        }
                    }
                    Type::Null | Type::Undefined => {}
                    _ => shape_unknown = true,
                }
                continue;
            }
        };
        // A key can be a plain identifier (`{ a: 1 }`), a string literal
        // (`{ "a": 1 }`), or a numeric literal (`{ 1: "x" }`); JavaScript stores
        // all three as string-keyed properties, so a numeric key is formatted the
        // same way its runtime string form would be. Anything else -- a computed
        // key (`{ [expr]: 1 }`) or a private name -- has no static name to give
        // this property, so it is skipped rather than guessed at.
        let name = match &property.key {
            PropertyKey::StaticIdentifier(key) => key.name.to_string(),
            PropertyKey::StringLiteral(key) => key.value.to_string(),
            PropertyKey::NumericLiteral(key) => key.value.to_string(),
            _ => continue,
        };
        let type_id = infer_expression_type(&property.value, scoping, ctx);
        let type_id = crate::types::widen(&ctx.arena, type_id);
        upsert_property(
            &mut properties,
            PropertyEntry {
                name: name.into(),
                type_id,
                optional: false,
                // `{ peek() {} }` prints as `{ peek(): any; }`, the way it was written.
                is_method: property.method,
            },
        );
    }

    if shape_unknown {
        return ctx.arena.error();
    }
    ctx.arena.alloc(Type::Object(ObjectType::new(properties)))
}

// Keeps names unique, which ObjectType's sorted merge-join relies on: a repeated
// key (`{ a: 1, a: 2 }`, or a spread followed by an explicit key) replaces the
// earlier entry in place rather than adding a second one.
fn upsert_property(properties: &mut Vec<PropertyEntry>, entry: PropertyEntry) {
    match properties.iter_mut().find(|p| p.name == entry.name) {
        Some(existing) => *existing = entry,
        None => properties.push(entry),
    }
}

// Each element's type is widened (a literal 5 becomes number) before being added
// to the element-type union, matching how [1, 2, 3] should infer as number[], not
// a union of three numeric literal types. An empty array literal has nothing to
// infer an element type from, so it is never[]: the type with no elements, assignable to
// an array of anything, which is what makes `const xs: number[] = []`, `return []` and
// `f([])` valid. A variable declared without an annotation widens it to any[] (see
// types::widen), because tsc treats `const xs = []` as an array that fills up later.
pub(super) fn infer_array_expression_type(
    array: &oxc_ast::ast::ArrayExpression,
    scoping: &Scoping,
    ctx: &mut CheckContext<'_, '_>,
) -> TypeId {
    let mut element_types = Vec::with_capacity(array.elements.len());

    for element in &array.elements {
        // Problem: `[...xs, 1]` ignored the spread, so the element type left out
        // whatever xs held and a later assignment was checked against a type that
        // was too narrow.
        // Picked: a spread of an array contributes that array's element type. A
        // spread of anything else (any, an error, a tuple-like or iterable type this
        // checker does not model) contributes any, which keeps the result usable
        // without guessing a shape.
        // Cost: iterables such as a Set or a string spread to any, not their item type.
        if let oxc_ast::ast::ArrayExpressionElement::SpreadElement(spread) = element {
            let spread_type = infer_expression_type(&spread.argument, scoping, ctx);
            let item = match ctx.arena.get(spread_type) {
                Type::Array(item) => *item,
                _ => ctx.arena.any(),
            };
            let item = crate::types::widen(&ctx.arena, item);
            if !element_types.contains(&item) {
                element_types.push(item);
            }
            continue;
        }
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
        0 => ctx.arena.never(),
        1 => element_types[0],
        _ => ctx.arena.alloc_union(element_types),
    };

    ctx.arena.alloc(Type::Array(element_type))
}
