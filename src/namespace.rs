//! Named types: `type Foo = ...` and `interface Foo { ... }`. Kept
//! separate from `SymbolTypeMap` because TypeScript keeps a type namespace
//! and a value namespace apart: `type Foo = ...` and `const Foo = ...`
//! don't collide.
//!
//! Every name resolves lazily, on first use, rather than at declaration
//! time. This is what lets one interface or alias reference another
//! declared later in the same file: `type A = B; interface B { x: number }`
//! only works if resolving `A` doesn't require `B` to already be built.
//! Resolving is cheap and side-effect-free, so there's nothing to gain by
//! special-casing "simple" aliases (a bare `type Foo = number`) to resolve
//! eagerly instead. Doing that would only reintroduce the exact ordering
//! problem lazy resolution exists to avoid.

use oxc_ast::ast::{Class, ClassElement, Expression, MethodDefinitionKind, PropertyKey, TSInterfaceDeclaration, TSType};

use crate::arena::{TypeArena, TypeId};
use crate::fxhash::FxHashMap;
use crate::type_annotation::{resolve_function_params, resolve_object_members, resolve_ts_type, resolve_type_annotation};
use crate::types::{ObjectType, PropertyEntry, Type};

#[derive(Clone, Copy)]
enum DeclKind<'a> {
    TypeAlias(&'a TSType<'a>),
    Interface(&'a TSInterfaceDeclaration<'a>),
    Class(&'a Class<'a>),
}

struct TypeEntry<'a> {
    kind: DeclKind<'a>,
    resolved: Option<TypeId>,
    /// Set while this entry's resolution is in progress, so a name that
    /// refers back to itself, directly or through another alias, is
    /// caught instead of recursing forever.
    resolving: bool,
}

pub struct TypeNamespace<'a> {
    // Names (`type`/`interface`/`class` identifiers) are arbitrary
    // strings, not a dense sequential index like `SymbolId` — unlike
    // `SymbolTypeMap`/`NarrowState`, there's no way to sidestep hashing
    // here, so a fast non-cryptographic hasher is the actual right fix
    // rather than a `Vec`. See `crate::fxhash` for why SipHash's
    // DoS-resistance isn't worth paying for on compiler-internal keys.
    entries: FxHashMap<String, TypeEntry<'a>>,
}

pub enum Resolution {
    Resolved(TypeId),
    /// The name refers back to itself with no base case. Recursive named
    /// types aren't supported yet. This is a case for an `Unsupported`
    /// diagnostic, not a checker bug.
    Circular,
    NotFound,
}

impl<'a> TypeNamespace<'a> {
    pub fn new() -> Self {
        Self { entries: FxHashMap::default() }
    }

    pub fn insert_type_alias(&mut self, name: &str, body: &'a TSType<'a>) {
        self.entries.insert(name.to_string(), TypeEntry { kind: DeclKind::TypeAlias(body), resolved: None, resolving: false });
    }

    pub fn insert_interface(&mut self, name: &str, decl: &'a TSInterfaceDeclaration<'a>) {
        self.entries.insert(name.to_string(), TypeEntry { kind: DeclKind::Interface(decl), resolved: None, resolving: false });
    }

    pub fn insert_class(&mut self, name: &str, class: &'a Class<'a>) {
        self.entries.insert(name.to_string(), TypeEntry { kind: DeclKind::Class(class), resolved: None, resolving: false });
    }

    pub fn resolve(&mut self, name: &str, arena: &mut TypeArena) -> Resolution {
        let Some(entry) = self.entries.get(name) else { return Resolution::NotFound };

        if let Some(type_id) = entry.resolved {
            return Resolution::Resolved(type_id);
        }
        if entry.resolving {
            return Resolution::Circular;
        }

        self.entries.get_mut(name).expect("checked above").resolving = true;

        let kind = self.entries.get(name).expect("checked above").kind;
        let resolved = match kind {
            DeclKind::TypeAlias(body) => resolve_ts_type(body, self, arena),
            DeclKind::Interface(decl) => resolve_object_members(&decl.body.body, self, arena),
            DeclKind::Class(class) => self.resolve_class(class, arena),
        };

        let entry = self.entries.get_mut(name).expect("still present");
        entry.resolving = false;

        match resolved {
            Some(type_id) => {
                entry.resolved = Some(type_id);
                Resolution::Resolved(type_id)
            }
            None => Resolution::NotFound,
        }
    }

    /// Builds a class's instance type: the parent's fields and methods
    /// (resolved the same lazy way, so `class B extends A` and
    /// `class A extends B` both work regardless of file order, the same
    /// as interfaces), with the class's own members added on top and
    /// overriding anything the parent had under the same name.
    ///
    /// Static members, getters/setters, and index signatures are skipped
    /// individually, since they're never part of the instance shape at
    /// all. A field or method that *is* part of the instance shape but
    /// isn't fully annotated fails the whole class instead of just that
    /// member, same policy as `resolve_object_members` for interfaces: a
    /// half-built object type would be worse than an honest unresolved
    /// one, since every caller (a variable typed as this class, a `new`
    /// call, an assignment) would otherwise silently check against an
    /// incomplete shape with no indication anything was skipped.
    fn resolve_class(&mut self, class: &'a Class<'a>, arena: &mut TypeArena) -> Option<TypeId> {
        let mut properties: Vec<PropertyEntry> = Vec::new();

        if let Some(heritage) = &class.heritage {
            if let Expression::Identifier(parent_name) = &heritage.expression {
                if let Resolution::Resolved(parent_type) = self.resolve(&parent_name.name, arena) {
                    if let Type::Object(parent_object) = arena.get(parent_type) {
                        properties = parent_object.properties.clone();
                    }
                }
                // A circular or unresolved parent just means no inherited
                // properties, not a failure of the whole class, the
                // class's own members still get checked.
            }
            // A superclass expression more complex than a bare name
            // (e.g. a mixin call) isn't resolved; inheritance is skipped.
        }

        for element in &class.body.body {
            match element {
                ClassElement::PropertyDefinition(prop) if !prop.r#static => {
                    let PropertyKey::StaticIdentifier(key) = &prop.key else { return None };
                    let Some(annotation) = &prop.type_annotation else { return None };
                    let Some(type_id) = resolve_type_annotation(annotation, self, arena) else { return None };
                    upsert_property(&mut properties, key.name.to_string(), type_id, prop.optional);
                }
                ClassElement::MethodDefinition(method)
                    if !method.r#static && method.kind == MethodDefinitionKind::Method =>
                {
                    let PropertyKey::StaticIdentifier(key) = &method.key else { return None };
                    let Some(return_type) =
                        method.value.return_type.as_ref().and_then(|rt| resolve_type_annotation(rt, self, arena))
                    else {
                        return None;
                    };
                    // An untyped parameter is a narrower gap than a
                    // missing return type (handled above — still fails
                    // the whole class, since there'd be nothing sound to
                    // record for this method's type at all): the
                    // method's presence, name, and return type are all
                    // still known here, only its argument types aren't.
                    // Registering it with `is_untyped`, the same
                    // mechanism `declare.rs` already uses for a class
                    // whose *constructor* has an untyped parameter,
                    // keeps every other correctly-typed member of this
                    // class checkable instead of discarding all of it
                    // over one loosely-typed method. `check_callable`
                    // (bridge/expressions.rs) is what actually reads
                    // this flag to skip arity checking on calls to it.
                    let (params, is_untyped) = match resolve_function_params(&method.value, self, arena) {
                        Some(params) => (params, false),
                        None => (Vec::new(), true),
                    };
                    let method_type = arena.alloc(Type::Function(crate::types::FunctionType {
                        params,
                        return_type,
                        is_untyped,
                    }));
                    upsert_property(&mut properties, key.name.to_string(), method_type, false);
                }
                // Constructors, getters/setters, static members, static
                // blocks, index signatures, and accessor properties aren't
                // part of the instance shape checked here.
                _ => {}
            }
        }

        properties.sort_by(|a, b| a.name.cmp(&b.name));
        Some(arena.alloc(Type::Object(ObjectType { properties })))
    }
}

/// Child members override a same-named parent member rather than
/// duplicating it.
fn upsert_property(properties: &mut Vec<PropertyEntry>, name: String, type_id: TypeId, optional: bool) {
    match properties.iter_mut().find(|p| p.name == name) {
        Some(existing) => {
            existing.type_id = type_id;
            existing.optional = optional;
        }
        None => properties.push(PropertyEntry { name, type_id, optional }),
    }
}

impl Default for TypeNamespace<'_> {
    fn default() -> Self {
        Self::new()
    }
}
