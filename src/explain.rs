//! Why a type is not assignable to another, in the shape tsc reports it.
//!
//! tsc does not print a bare "Type 'A' is not assignable to type 'B'." It prints a
//! chain: that line, then the reason it holds one level down (a union member that
//! fails, a property that is missing or has the wrong type, a parameter that is not
//! accepted), each level indented two spaces under the one above.
//!
//! The chain is built only after `is_subtype` has already said "no", so nothing here
//! decides assignability. A level that cannot be worked out is left off, which makes
//! the message shorter than tsc's and never wrong about whether there is an error.
//!
//! Rules this follows, each checked against `tsc` itself:
//!
//! - A union source names its first failing member. tsc keeps union members in type-id
//!   order, in which `undefined` and `null` come before `string`, then `number`,
//!   `boolean`, then everything else in creation order. That is why `number | null`
//!   against `number` names `null`, whatever order the union is printed in.
//! - A union target is not elaborated.
//! - Arrays and generic applications of the same declaration name the first element or
//!   type argument that fails, and nothing about the structure in between.
//! - A missing property is reported as one line (`Property 'y' is missing in type ...`)
//!   for one, or as a list for several (five at most are named; with more, the first
//!   four and a count). When that line is the only reason, the "is not assignable"
//!   line above it is dropped, so it becomes the top message and takes its code.
//! - A property of the wrong type adds `Types of property 'p' are incompatible.`, and a
//!   parameter adds `Types of parameters 'a' and 'b' are incompatible.` (the target's
//!   parameter type is the one that must be assignable to the source's).

use crate::arena::{TypeArena, TypeId};
use crate::diagnostic_codes::DiagnosticCode;
use crate::subtyping::{is_subtype, param_type_at, property_relates};
use crate::type_display::{display_source_type, display_type};
use crate::types::{FunctionType, ObjectType, Type};

// A chain this deep is a recursive type being walked; tsc itself stops elaborating
// long before this.
const MAX_DEPTH: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    NotAssignable,
    Missing(DiagnosticCode),
    Detail,
}

/// One line of a chain.
#[derive(Clone, Debug)]
pub struct Level {
    kind: Kind,
    text: String,
}

impl Level {
    fn not_assignable(arena: &TypeArena, source: TypeId, target: TypeId) -> Self {
        let shown_target = Self::non_nullable_target(arena, source, target);
        Self {
            kind: Kind::NotAssignable,
            text: format!(
                "Type '{}' is not assignable to type '{}'.",
                display_source_type(arena, source, shown_target),
                display_type(arena, shown_target)
            ),
        }
    }

    fn detail(text: String) -> Self {
        Self {
            kind: Kind::Detail,
            text,
        }
    }

    fn is_missing(&self) -> bool {
        matches!(self.kind, Kind::Missing(_))
    }

    /// tsc reports against the one type that is left when a source that is not null or
    /// undefined is compared with `T | null` or `T | undefined`: `"x"` into `Node | null` is
    /// "not assignable to type 'Node'". With more than one type left (`A | B | null`) the
    /// whole union is named, and so it is when the source can itself be null or undefined.
    fn non_nullable_target(arena: &TypeArena, source: TypeId, target: TypeId) -> TypeId {
        let Type::Union(members) = arena.get(target) else {
            return target;
        };
        let nullish = |id: TypeId| matches!(arena.get(id), Type::Null | Type::Undefined);
        let source_nullish = match arena.get(source) {
            Type::Union(parts) => parts.iter().any(|&p| nullish(p)),
            _ => nullish(source),
        };
        if source_nullish {
            return target;
        }
        let mut rest = members.iter().copied().filter(|&m| !nullish(m));
        match (rest.next(), rest.next()) {
            (Some(only), None) if members.iter().any(|&m| nullish(m)) => only,
            _ => target,
        }
    }

    /// A level that opens a message of its own, such as `Argument of type ...`.
    pub fn headline(text: String) -> Self {
        Self::detail(text)
    }
}

/// The whole chain for `source` not being assignable to `target`, head included.
/// When the only reason is a missing property, that line is the head.
pub fn mismatch_levels(arena: &TypeArena, source: TypeId, target: TypeId) -> Vec<Level> {
    reason(arena, source, target, 0)
}

/// The reasons under a head the caller writes itself (`Argument of type ...`,
/// `Type ... does not satisfy the constraint ...`), without the "is not assignable"
/// line that head replaces.
pub fn mismatch_children(arena: &TypeArena, source: TypeId, target: TypeId) -> Vec<Level> {
    children(arena, source, target, 0)
}

/// The code of the top line: a missing-property line carries its own, anything else
/// is the code the caller reports a plain mismatch under.
pub fn top_code(levels: &[Level], plain: DiagnosticCode) -> DiagnosticCode {
    match levels.first().map(|level| level.kind) {
        Some(Kind::Missing(code)) => code,
        _ => plain,
    }
}

/// tsc's flattening: each level on its own line, two spaces deeper than the last.
pub fn render(levels: &[Level]) -> String {
    let mut out = String::new();
    for (depth, level) in levels.iter().enumerate() {
        if depth > 0 {
            out.push('\n');
            for _ in 0..depth {
                out.push_str("  ");
            }
        }
        out.push_str(&level.text);
    }
    out
}

fn reason(arena: &TypeArena, source: TypeId, target: TypeId, depth: usize) -> Vec<Level> {
    let below = children(arena, source, target, depth);
    // A lone missing property is promoted to the head (tsc's TS2741) only for a plain
    // object target. For `A & B` tsc keeps "not assignable to 'A & B'" as the head and puts
    // the missing property beneath it, so the promotion is skipped there.
    let target_is_intersection = matches!(arena.get(target), Type::Intersection(_));
    if !target_is_intersection && below.first().is_some_and(Level::is_missing) {
        return below;
    }
    let mut levels = vec![Level::not_assignable(arena, source, target)];
    levels.extend(below);
    levels
}

fn children(arena: &TypeArena, source: TypeId, target: TypeId, depth: usize) -> Vec<Level> {
    if depth >= MAX_DEPTH {
        return Vec::new();
    }
    match (arena.get(source), arena.get(target)) {
        // An enum is a primitive to tsc and a mismatch below it is not elaborated.
        (Type::Union(_), _) if arena.is_enum(source) => Vec::new(),
        // Problem: a mismatch against `A & B` stopped at "not assignable to 'A & B'", where
        // tsc goes on to say which part of the intersection failed.
        // Picked: assignable to an intersection means assignable to every member, so the
        // reason is the first member the source does not satisfy, explained as if that
        // member alone were the target (a plain "not assignable" line, or the missing
        // property it lacks). A union source is left to the arms below, which already
        // explain it member by member.
        (source_type, Type::Intersection(members)) if !matches!(source_type, Type::Union(_)) => {
            match members
                .iter()
                .copied()
                .find(|&member| !crate::subtyping::is_subtype(arena, source, member))
            {
                Some(failing) => reason(arena, source, failing, depth + 1),
                None => Vec::new(),
            }
        }
        // Nothing but the parameter itself (or never/any, which never get here) is
        // assignable to a type parameter, since it could be instantiated as anything.
        // tsc says so, and says which of two things is true of the constraint.
        (_, Type::GenericParameter(_, name, bound)) => {
            let shown = display_source_type(arena, source, target);
            let text = match bound {
                Some(bound) if is_subtype(arena, source, *bound) => format!(
                    "'{shown}' is assignable to the constraint of type '{name}', but '{name}' could be instantiated with a different subtype of constraint '{}'.",
                    display_type(arena, *bound)
                ),
                _ => format!(
                    "'{name}' could be instantiated with an arbitrary type which could be unrelated to '{shown}'."
                ),
            };
            vec![Level::detail(text)]
        }
        (Type::Union(members), _) => {
            // `string | "fallback"` is just `string` once tsc has reduced it, so there
            // is no member to point at.
            if distinct_after_reduction(arena, members) <= 1 {
                return Vec::new();
            }
            let mut ordered = members.clone();
            ordered.sort_by_key(|&member| tsc_rank(arena, member));
            match ordered
                .into_iter()
                .find(|&member| !is_subtype(arena, member, target))
            {
                Some(member) => reason(arena, member, target, depth + 1),
                None => Vec::new(),
            }
        }
        (_, Type::Union(_)) => Vec::new(),
        (Type::Array(from), Type::Array(to)) => reason(arena, *from, *to, depth + 1),
        (Type::Object(from), Type::Object(to)) => {
            if let (Some((from_name, from_args)), Some((to_name, to_args))) =
                (arena.app_parts(source), arena.app_parts(target))
                && from_name == to_name
                && from_args.len() == to_args.len()
                && let Some(index) =
                    (0..from_args.len()).find(|&i| !is_subtype(arena, from_args[i], to_args[i]))
            {
                return reason(arena, from_args[index], to_args[index], depth + 1);
            }
            object_children(arena, source, target, from, to, depth)
        }
        (Type::Function(from), Type::Function(to)) => function_children(arena, from, to, depth),
        _ => Vec::new(),
    }
}

// How many members a union has once a literal is absorbed by its own base type.
fn distinct_after_reduction(arena: &TypeArena, members: &[TypeId]) -> usize {
    let has = |wanted: fn(&Type) -> bool| members.iter().any(|&m| wanted(arena.get(m)));
    let (string, number, boolean) = (
        has(|t| matches!(t, Type::String)),
        has(|t| matches!(t, Type::Number)),
        has(|t| matches!(t, Type::Boolean)),
    );
    members
        .iter()
        .filter(|&&m| {
            !matches!(
                (arena.get(m), string, number, boolean),
                (Type::StringLiteral(_), true, _, _)
                    | (Type::NumberLiteral(_), _, true, _)
                    | (Type::BooleanLiteral(_), _, _, true)
            )
        })
        .count()
}

// The order tsc compares union members in, which is the order of their type ids.
fn tsc_rank(arena: &TypeArena, member: TypeId) -> u8 {
    match arena.get(member) {
        Type::Undefined => 0,
        Type::Null => 1,
        Type::String => 2,
        Type::Number => 3,
        Type::Boolean | Type::BooleanLiteral(_) => 4,
        _ => 5,
    }
}

fn object_children(
    arena: &TypeArena,
    source: TypeId,
    target: TypeId,
    from: &ObjectType,
    to: &ObjectType,
    depth: usize,
) -> Vec<Level> {
    let missing: Vec<&str> = to
        .properties
        .iter()
        .filter(|wanted| {
            !wanted.optional && !from.properties.iter().any(|have| have.name == wanted.name)
        })
        .map(|wanted| &*wanted.name)
        .collect();
    if !missing.is_empty() {
        return vec![missing_level(arena, source, target, &missing)];
    }

    for wanted in to.properties.iter() {
        let Some(have) = from.properties.iter().find(|p| p.name == wanted.name) else {
            continue;
        };
        if have.optional && !wanted.optional {
            return vec![Level::detail(format!(
                "Property '{}' is optional in type '{}' but required in type '{}'.",
                wanted.name,
                display_type(arena, source),
                display_type(arena, target)
            ))];
        }
        if !property_relates(arena, have.type_id, wanted) {
            let mut levels = vec![Level::detail(format!(
                "Types of property '{}' are incompatible.",
                wanted.name
            ))];
            levels.extend(reason(arena, have.type_id, wanted.type_id, depth + 1));
            return levels;
        }
    }
    Vec::new()
}

fn missing_level(arena: &TypeArena, source: TypeId, target: TypeId, missing: &[&str]) -> Level {
    let (from, to) = (display_type(arena, source), display_type(arena, target));
    match missing {
        [only] => Level {
            kind: Kind::Missing(DiagnosticCode::MissingProperty),
            text: format!(
                "Property '{only}' is missing in type '{from}' but required in type '{to}'."
            ),
        },
        names if names.len() <= 5 => Level {
            kind: Kind::Missing(DiagnosticCode::MissingProperties),
            text: format!(
                "Type '{from}' is missing the following properties from type '{to}': {}",
                names.join(", ")
            ),
        },
        names => Level {
            kind: Kind::Missing(DiagnosticCode::MissingPropertiesMany),
            text: format!(
                "Type '{from}' is missing the following properties from type '{to}': {}, and {} more.",
                names[..4].join(", "),
                names.len() - 4
            ),
        },
    }
}

fn function_children(
    arena: &TypeArena,
    from: &FunctionType,
    to: &FunctionType,
    depth: usize,
) -> Vec<Level> {
    let positions = from.params.len().max(to.params.len());
    for position in 0..positions {
        let (Some(have), Some(wanted)) = (
            param_type_at(arena, &from.params, position),
            param_type_at(arena, &to.params, position),
        ) else {
            continue;
        };
        if is_subtype(arena, wanted, have) {
            continue;
        }
        let name_of = |params: &[crate::types::Param]| {
            params
                .get(position)
                .or(params.last())
                .and_then(|param| param.name.as_deref().map(str::to_owned))
                .unwrap_or_else(|| format!("arg{position}"))
        };
        let mut levels = vec![Level::detail(format!(
            "Types of parameters '{}' and '{}' are incompatible.",
            name_of(&from.params),
            name_of(&to.params)
        ))];
        levels.extend(reason(arena, wanted, have, depth + 1));
        return levels;
    }
    if !is_subtype(arena, from.return_type, to.return_type) {
        return reason(arena, from.return_type, to.return_type, depth + 1);
    }
    Vec::new()
}
