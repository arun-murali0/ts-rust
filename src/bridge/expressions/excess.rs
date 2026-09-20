use oxc_ast::ast::{Expression, ObjectPropertyKind, PropertyKey};

use crate::arena::TypeId;
use crate::types::Type;

use super::super::context::CheckContext;

// TypeScript's excess property check: a *fresh* object literal, one written
// directly where a target type is expected, may not carry a property the target
// does not declare. The same object assigned through a variable first is no
// longer fresh, which is why this inspects the expression itself rather than
// living in structural subtyping (where `{ x, y }` is, correctly, a subtype of
// `{ x }`).
//
// Called only after the ordinary assignability check has already passed, so a
// literal that is wrong in some other way reports that one error, not two.
// Applies at the three places a literal meets an expected type today: a
// variable's declared type, a call argument, and a return statement. Recurses
// into nested literals whose target property is itself an object type. A target
// that is not a plain object type (a union, an unresolved generic parameter) is
// skipped, which is always safe, just less precise.
pub(crate) fn check_excess_properties(
    expr: &Expression,
    target: TypeId,
    ctx: &mut CheckContext<'_, '_>,
) {
    let Expression::ObjectExpression(object) = expr else {
        return;
    };
    let Type::Object(target_object) = ctx.arena.get(target).clone() else {
        return;
    };

    for property in &object.properties {
        // Spreads and computed keys carry no statically known name to check.
        let ObjectPropertyKind::ObjectProperty(property) = property else {
            continue;
        };
        let PropertyKey::StaticIdentifier(key) = &property.key else {
            continue;
        };

        match target_object
            .properties
            .iter()
            .find(|p| *p.name == *key.name.as_str())
        {
            None => ctx.error(
                crate::diagnostic_messages::messages::excess_property(&key.name),
                key.span,
            ),
            Some(target_property) => {
                check_excess_properties(&property.value, target_property.type_id, ctx);
            }
        }
    }
}
