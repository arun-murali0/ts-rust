use oxc_ast::ast::{IdentifierReference, Program, TSTypeName, TSTypeReference};
use oxc_ast_visit::{Visit, walk};
use oxc_semantic::Scoping;

use crate::lib_global_types::is_lib_global_type;
use crate::namespace::TypeNamespace;

use super::context::CheckContext;
use super::narrow::resolve_symbol_id;

// Problem: an unknown name in a type annotation was reported only where a variable or a
// parameter happened to carry it. `type G = A & Missing`, `interface I { m: Missing }`,
// `function f(): Missing`, `class C { x: Missing }`, `Array<Missing>` and `A | Missing`
// in a parameter all passed in silence, or became a "could not be resolved" warning
// when something used them, where tsc says "Cannot find name 'Missing'." (TS2304)
// at the name.
// Picked: one pass over every type reference in the file, after the declarations are
// known, reporting each name that resolves to nothing: no symbol in scope (which covers
// type parameters, imports and nested declarations), no top-level type in the
// namespace, and no global type the standard library provides. Each name is reported
// once, at its own position, so `Missing7 & Missing8` is two errors, as in tsc.
// Cost: a type reference written as a qualified name (`Intl.DateTimeFormat`) is left
// alone, since its first part is a namespace and not a type, and `typeof x` in a type
// is a value reference this pass does not look at.
pub(crate) fn report_unresolved_type_names<'ast>(
    program: &Program<'ast>,
    scoping: &Scoping,
    ctx: &mut CheckContext<'ast, '_>,
) {
    let mut walker = Walker { scoping, ctx };
    walker.visit_program(program);
}

// True when `id` names no type anywhere: not declared in scope, not a top-level type of
// this file, and not one of the library's global types (which ts-rust does not model yet,
// so they are unsupported, not missing).
pub(crate) fn is_unresolved_type_name(
    id: &IdentifierReference,
    scoping: &Scoping,
    namespace: &TypeNamespace,
) -> bool {
    resolve_symbol_id(id, scoping).is_none()
        && !namespace.contains(&id.name)
        && !is_lib_global_type(&id.name)
}

struct Walker<'s, 'c, 'ast, 'src> {
    scoping: &'s Scoping,
    ctx: &'c mut CheckContext<'ast, 'src>,
}

impl<'a> Visit<'a> for Walker<'_, '_, '_, '_> {
    fn visit_ts_type_reference(&mut self, it: &TSTypeReference<'a>) {
        if let TSTypeName::IdentifierReference(id) = &it.type_name
            && is_unresolved_type_name(id, self.scoping, &self.ctx.namespace)
        {
            self.ctx.error(
                crate::diagnostic_messages::messages::unresolved_identifier(&id.name),
                id.span,
            );
        }
        // Type arguments (`Array<Missing>`) are references too.
        walk::walk_ts_type_reference(self, it);
    }
}
