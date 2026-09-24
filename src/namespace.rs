use oxc_ast::ast::{
    Class, ClassElement, Expression, MethodDefinitionKind, PropertyKey, TSInterfaceDeclaration,
    TSType,
};
use oxc_span::{GetSpan, Span};

use crate::arena::{TypeArena, TypeId};
use crate::fxhash::FxHashMap;
use crate::type_annotation::{
    resolve_function_params, resolve_object_members, resolve_ts_type, resolve_type_annotation,
};
use crate::types::{ObjectType, PropertyEntry, Type, TypeParameterId};

// A type namespace is a single flat map from name to declaration, not a scope tree.
// There is no block or module scoping for types in this checker; every top-level
// interface, type alias, and class shares one namespace. Generic type parameters
// are fit into this same flat model by temporarily shadowing a name (see
// push_type_params below) rather than motivating a real scope-tree rewrite.
#[derive(Clone, Copy)]
enum DeclKind<'a> {
    TypeAlias(
        &'a TSType<'a>,
        Option<&'a oxc_ast::ast::TSTypeParameterDeclaration<'a>>,
    ),
    Interface(&'a TSInterfaceDeclaration<'a>),
    Class(&'a Class<'a>),

    Resolved,
}

// Opaque token returned by push_type_params and consumed by pop_type_params.
// Carries whatever each shadowed name previously pointed to, so restoring is
// self-contained and does not need the caller to remember which names were pushed.
pub struct TypeParamScope<'a>(Vec<(String, Option<TypeEntry<'a>>)>);

struct TypeEntry<'a> {
    kind: DeclKind<'a>,
    resolved: Option<TypeId>,

    resolving: bool,
}

// A type reference whose type argument count disagrees with the declaration it
// names. Recorded during type resolution, which has no access to the diagnostics
// list, and drained into real diagnostics once checking finishes (see
// bridge::check_program), the same way implicit_any_params is.
pub struct TypeArgumentIssue {
    pub name: String,
    // Fewest type arguments a reference must give: parameters without a default.
    pub required: usize,
    // Most it may give: every declared parameter.
    pub expected: usize,
    pub given: usize,
    pub span: Span,
}

pub struct TypeNamespace<'a> {
    entries: FxHashMap<String, TypeEntry<'a>>,

    // Keyed by TypeParameterId (source-position-derived, see that type in types.rs)
    // rather than by name or by an AST pointer, so a generic function's signature,
    // resolved once up front, and its body, resolved later in a separate pass,
    // agree on the exact same GenericParameter node for T. Without this, a T[]
    // annotation inside the body would never match the parameter T came from.
    type_param_cache: FxHashMap<TypeParameterId, TypeId>,

    // Untyped parameters found inside function *type annotations* such as
    // `(x) => number`. Type resolution has no access to the diagnostics list, so
    // they are collected here, deduplicated by source position (the same
    // annotation can be resolved more than once), and drained into real
    // diagnostics once checking finishes. See bridge::check_program.
    implicit_any_params: Vec<(String, Span)>,

    // Type parameters whose `extends` bound could not be resolved. The parameter
    // is then treated as unconstrained, which silently loses both the call-site
    // check and the use of the bound's members in the body, so it is recorded
    // here and reported as a warning once checking finishes, the same way
    // implicit_any_params is.
    unresolved_constraints: Vec<(String, Span)>,

    // See TypeArgumentIssue. Deduplicated by source position, since the same
    // annotation can be resolved more than once.
    type_argument_issues: Vec<TypeArgumentIssue>,

    // Type arguments that do not satisfy the `extends` bound of the parameter
    // they were given to (`Box<number>` for `Box<T extends string>`): the
    // parameter's name and the argument's span. Collected here for the same
    // reason as the issues above and deduplicated the same way.
    constraint_violations: Vec<(String, Span)>,
}

pub enum Resolution {
    Resolved(TypeId),

    Circular,
    NotFound,
}

impl<'a> TypeNamespace<'a> {
    pub fn new() -> Self {
        Self {
            entries: FxHashMap::default(),
            type_param_cache: FxHashMap::default(),
            implicit_any_params: Vec::new(),
            unresolved_constraints: Vec::new(),
            type_argument_issues: Vec::new(),
            constraint_violations: Vec::new(),
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    pub fn note_implicit_any_params(&mut self, params: &oxc_ast::ast::FormalParameters) {
        for param in &params.items {
            if param.type_annotation.is_some() {
                continue;
            }
            let oxc_ast::ast::BindingPattern::BindingIdentifier(id) = &param.pattern else {
                continue;
            };
            if self
                .implicit_any_params
                .iter()
                .any(|(_, span)| span.start == id.span.start)
            {
                continue;
            }
            self.implicit_any_params
                .push((id.name.to_string(), id.span));
        }
    }

    pub fn take_implicit_any_params(&mut self) -> Vec<(String, Span)> {
        std::mem::take(&mut self.implicit_any_params)
    }

    pub fn take_unresolved_constraints(&mut self) -> Vec<(String, Span)> {
        std::mem::take(&mut self.unresolved_constraints)
    }

    // The `<...>` list declared by the interface or type alias called `name`.
    // Outer None: not something whose type parameters are modelled (a class,
    // whose generics are not supported yet, a type parameter, an enum, or an
    // unknown name), so nothing is ever reported against it and an unsupported
    // feature cannot produce a false error. Inner None: a non-generic one.
    fn declared_type_param_list(
        &self,
        name: &str,
    ) -> Option<Option<&'a oxc_ast::ast::TSTypeParameterDeclaration<'a>>> {
        match self.entries.get(name)?.kind {
            DeclKind::TypeAlias(_, type_params) => Some(type_params),
            DeclKind::Interface(decl) => Some(decl.type_parameters.as_deref()),
            DeclKind::Class(_) | DeclKind::Resolved => None,
        }
    }

    // (required, total) type argument counts. A parameter with a default may be
    // omitted, which is why these differ; tsc accepts `Pair<number>` for
    // `interface Pair<A, B = string>`.
    pub fn declared_type_param_arity(&self, name: &str) -> Option<(usize, usize)> {
        Some(match self.declared_type_param_list(name)? {
            None => (0, 0),
            Some(decl) => (
                decl.params
                    .iter()
                    .filter(|param| param.default.is_none())
                    .count(),
                decl.params.len(),
            ),
        })
    }

    // The declaration itself when it is generic, so a reference can bind one
    // argument per *declared* parameter, in declaration order, instead of
    // guessing the order from whichever parameters the resolved body mentions.
    pub fn declared_type_param_decl(
        &self,
        name: &str,
    ) -> Option<&'a oxc_ast::ast::TSTypeParameterDeclaration<'a>> {
        self.declared_type_param_list(name)?
            .filter(|decl| !decl.params.is_empty())
    }

    // Resolves the `= Default` of the type parameter at `index`, with the
    // declaration's own parameters in scope so a default may mention an earlier
    // one (`<T, U = T>`). The result can still contain those GenericParameter
    // placeholders; the caller substitutes the arguments it already has.
    pub fn resolve_type_param_default(
        &mut self,
        arena: &mut TypeArena,
        decl: &'a oxc_ast::ast::TSTypeParameterDeclaration<'a>,
        index: usize,
    ) -> Option<TypeId> {
        let default = decl.params.get(index)?.default.as_ref()?;
        let scope = self.push_decl_type_params(arena, Some(decl));
        let resolved = resolve_ts_type(default, self, arena);
        self.pop_type_params(scope);
        resolved
    }

    pub fn note_type_argument_issue(
        &mut self,
        name: &str,
        required: usize,
        expected: usize,
        given: usize,
        span: Span,
    ) {
        if self
            .type_argument_issues
            .iter()
            .any(|issue| issue.span.start == span.start)
        {
            return;
        }
        self.type_argument_issues.push(TypeArgumentIssue {
            name: name.to_string(),
            required,
            expected,
            given,
            span,
        });
    }

    // The resolved `extends` bound of a declared type parameter, read back from
    // the GenericParameter node push_decl_type_params cached for it. None when the
    // parameter is unconstrained, or its bound could not be resolved (already
    // reported as a warning), so nothing is ever enforced against it.
    pub fn type_param_constraint(&self, arena: &TypeArena, id: TypeParameterId) -> Option<TypeId> {
        let node = *self.type_param_cache.get(&id)?;
        match arena.get(node) {
            Type::GenericParameter(_, _, constraint) => *constraint,
            _ => None,
        }
    }

    pub fn note_constraint_violation(&mut self, parameter_name: &str, span: Span) {
        if self
            .constraint_violations
            .iter()
            .any(|(_, existing)| existing.start == span.start)
        {
            return;
        }
        self.constraint_violations
            .push((parameter_name.to_string(), span));
    }

    pub fn take_constraint_violations(&mut self) -> Vec<(String, Span)> {
        std::mem::take(&mut self.constraint_violations)
    }

    pub fn take_type_argument_issues(&mut self) -> Vec<TypeArgumentIssue> {
        std::mem::take(&mut self.type_argument_issues)
    }

    pub fn insert_type_alias(
        &mut self,
        name: &str,
        body: &'a TSType<'a>,
        type_parameters: Option<&'a oxc_ast::ast::TSTypeParameterDeclaration<'a>>,
    ) {
        self.entries.insert(
            name.to_string(),
            TypeEntry {
                kind: DeclKind::TypeAlias(body, type_parameters),
                resolved: None,
                resolving: false,
            },
        );
    }

    pub fn insert_interface(&mut self, name: &str, decl: &'a TSInterfaceDeclaration<'a>) {
        self.entries.insert(
            name.to_string(),
            TypeEntry {
                kind: DeclKind::Interface(decl),
                resolved: None,
                resolving: false,
            },
        );
    }

    pub fn insert_class(&mut self, name: &str, class: &'a Class<'a>) {
        self.entries.insert(
            name.to_string(),
            TypeEntry {
                kind: DeclKind::Class(class),
                resolved: None,
                resolving: false,
            },
        );
    }

    pub fn insert_resolved(&mut self, name: &str, type_id: TypeId) {
        self.entries.insert(
            name.to_string(),
            TypeEntry {
                kind: DeclKind::Resolved,
                resolved: Some(type_id),
                resolving: false,
            },
        );
    }

    // Temporarily shadows each of func's own type parameter names with a
    // GenericParameter node, so a generic function's signature and body can refer
    // to T and have it resolve here just like any other named type. The same node
    // is reused across repeated calls for the same function through
    // type_param_cache, and whatever a name previously pointed to is restored by
    // the matching pop_type_params call.
    //
    // `func` deliberately uses `'_` rather than `'a`: only names and spans are read
    // from it, nothing borrowed from the AST is stored. Tying it to `'a` would
    // force every caller to hold a Function with exactly the namespace's lifetime,
    // because `&mut TypeNamespace<'a>` is invariant in `'a`.
    pub fn push_type_params(
        &mut self,
        arena: &mut TypeArena,
        func: &oxc_ast::ast::Function<'_>,
    ) -> TypeParamScope<'a> {
        self.push_decl_type_params(arena, func.type_parameters.as_deref())
    }

    // The declaration-shaped generalization of push_type_params above: a
    // function's own `<T, ...>` list is one source of a type parameter
    // declaration, but a generic interface (`interface Box<T>`) or generic
    // type alias (`type Box<T> = ...`) needs the exact same shadow-then-resolve
    // treatment for its body to be able to refer to T. Both routes end up
    // allocating the same kind of GenericParameter node, keyed the same way by
    // TypeParameterId, so a interface's declared T and a type alias's declared
    // T are never confused with each other (different declaration_span_start)
    // even though both might be named "T".
    pub fn push_decl_type_params(
        &mut self,
        arena: &mut TypeArena,
        decl: Option<&oxc_ast::ast::TSTypeParameterDeclaration<'_>>,
    ) -> TypeParamScope<'a> {
        let Some(decl) = decl else {
            return TypeParamScope(Vec::new());
        };

        let mut saved = Vec::with_capacity(decl.params.len());
        for (index, param) in decl.params.iter().enumerate() {
            let name = param.name.name.to_string();
            let id = TypeParameterId::new(param.span().start, index as u32);

            // Resolved before touching the cache entry, not inside its
            // or_insert_with closure: resolving a constraint needs a full &mut
            // self (it can reference other named types), which would conflict
            // with the field-level borrow entry() already holds on
            // type_param_cache. Only resolved on a genuine cache miss, so a
            // constraint referencing something expensive is still only resolved
            // once per declaration, not once per push_type_params call.
            let type_id = match self.type_param_cache.get(&id).copied() {
                Some(cached) => cached,
                None => {
                    let constraint = param
                        .constraint
                        .as_ref()
                        .and_then(|c| crate::type_annotation::resolve_ts_type(c, self, arena));
                    if param.constraint.is_some()
                        && constraint.is_none()
                        && !self
                            .unresolved_constraints
                            .iter()
                            .any(|(_, span)| span.start == param.span().start)
                    {
                        self.unresolved_constraints
                            .push((name.clone(), param.span()));
                    }
                    let type_id = arena.alloc(Type::GenericParameter(id, name.clone(), constraint));
                    self.type_param_cache.insert(id, type_id);
                    type_id
                }
            };

            saved.push((name.clone(), self.entries.remove(&name)));
            self.insert_resolved(&name, type_id);
        }
        TypeParamScope(saved)
    }

    pub fn pop_type_params(&mut self, scope: TypeParamScope<'a>) {
        for (name, saved_entry) in scope.0 {
            match saved_entry {
                Some(entry) => {
                    self.entries.insert(name, entry);
                }
                None => {
                    self.entries.remove(&name);
                }
            }
        }
    }

    // Resolution is lazy and memoized: a declaration is only turned into a
    // concrete Type the first time something actually references it, and the
    // result is cached in entry.resolved so later references are free. The
    // resolving flag detects a declaration that references itself while still
    // being resolved (for example, two type aliases that refer to each other),
    // reporting it as Circular instead of recursing forever.
    pub fn resolve(&mut self, name: &str, arena: &mut TypeArena) -> Resolution {
        let Some(entry) = self.entries.get(name) else {
            return Resolution::NotFound;
        };

        if let Some(type_id) = entry.resolved {
            return Resolution::Resolved(type_id);
        }
        if entry.resolving {
            return Resolution::Circular;
        }

        let Some(entry_being_resolved) = self.entries.get_mut(name) else {
            return Resolution::NotFound;
        };
        entry_being_resolved.resolving = true;

        let Some(kind) = self.entries.get(name).map(|entry| entry.kind) else {
            return Resolution::NotFound;
        };

        // A generic interface or type alias's own `<T, ...>` list is pushed as a
        // shadow, exactly like a generic function's, before its body is resolved,
        // so `Box<T>`'s body can refer to T and have it resolve to the same
        // GenericParameter node a caller will later substitute via
        // type_annotation::resolve_ts_type's TSTypeReference arm. The resulting
        // resolved shape -- still containing bare GenericParameter placeholders
        // -- is what gets cached in entry.resolved; substitution for a specific
        // `Box<number>` happens later, per reference, against this one cached
        // generic shape, the same way a generic function's FunctionType is
        // resolved once and substituted per call.
        let type_params = match kind {
            DeclKind::TypeAlias(_, type_params) => type_params,
            DeclKind::Interface(decl) => decl.type_parameters.as_deref(),
            DeclKind::Class(_) | DeclKind::Resolved => None,
        };
        let scope = self.push_decl_type_params(arena, type_params);

        let resolved = match kind {
            DeclKind::TypeAlias(body, _) => resolve_ts_type(body, self, arena),
            DeclKind::Interface(decl) => resolve_object_members(&decl.body.body, self, arena),
            DeclKind::Class(class) => self.resolve_class(class, arena),
            DeclKind::Resolved => None,
        };
        self.pop_type_params(scope);

        let Some(entry) = self.entries.get_mut(name) else {
            return Resolution::NotFound;
        };
        entry.resolving = false;

        match resolved {
            Some(type_id) => {
                entry.resolved = Some(type_id);
                Resolution::Resolved(type_id)
            }
            None => Resolution::NotFound,
        }
    }

    // Builds a class's structural shape by first copying the parent's own already
    // resolved properties, then overriding or adding this class's own fields and
    // methods. This clones the parent's whole property list at every level of an
    // inheritance chain, so resolving a class costs work proportional to its own
    // depth in the hierarchy, not just its own member count. PropertyEntry's name
    // is an Rc<str> specifically so that repeated clone is a refcount bump rather
    // than a fresh heap allocation per property per level.
    fn resolve_class(&mut self, class: &'a Class<'a>, arena: &mut TypeArena) -> Option<TypeId> {
        let mut properties: Vec<PropertyEntry> = Vec::new();

        if let Some(heritage) = &class.heritage {
            if let Expression::Identifier(parent_name) = &heritage.expression {
                if let Resolution::Resolved(parent_type) = self.resolve(&parent_name.name, arena) {
                    if let Type::Object(parent_object) = arena.get(parent_type) {
                        properties = parent_object.properties.clone();
                    }
                }
            }
        }

        for element in &class.body.body {
            match element {
                // A computed field name, or any field/method this checker cannot
                // fully resolve, makes the whole class unresolvable rather than
                // just skipping that one member. Silently dropping a member would
                // let later code reference a property that source code clearly
                // declares but that structurally does not exist here, which is a
                // worse failure mode than an honest "this class isn't supported."
                ClassElement::PropertyDefinition(prop) if !prop.r#static => {
                    let PropertyKey::StaticIdentifier(key) = &prop.key else {
                        return None;
                    };
                    let annotation = prop.type_annotation.as_ref()?;
                    let type_id = resolve_type_annotation(annotation, self, arena)?;
                    upsert_property(
                        &mut properties,
                        key.name.to_string(),
                        type_id,
                        prop.optional,
                        false,
                    );
                }
                ClassElement::MethodDefinition(method)
                    if !method.r#static && method.kind == MethodDefinitionKind::Method =>
                {
                    let PropertyKey::StaticIdentifier(key) = &method.key else {
                        return None;
                    };
                    let return_type = method
                        .value
                        .return_type
                        .as_ref()
                        .and_then(|rt| resolve_type_annotation(rt, self, arena))?;

                    let (params, is_untyped) =
                        match resolve_function_params(&method.value.params, self, arena) {
                            Some(params) => (params, false),
                            None => (Vec::new(), true),
                        };
                    let method_type = arena.alloc(Type::Function(crate::types::FunctionType {
                        params,
                        return_type,
                        is_untyped,
                    }));
                    upsert_property(
                        &mut properties,
                        key.name.to_string(),
                        method_type,
                        false,
                        true,
                    );
                }

                _ => {}
            }
        }

        Some(arena.alloc(Type::Object(ObjectType::new(properties))))
    }
}

fn upsert_property(
    properties: &mut Vec<PropertyEntry>,
    name: String,
    type_id: TypeId,
    optional: bool,
    is_method: bool,
) {
    match properties.iter_mut().find(|p| *p.name == *name) {
        Some(existing) => {
            existing.type_id = type_id;
            existing.optional = optional;
            existing.is_method = is_method;
        }
        None => properties.push(PropertyEntry {
            name: name.into(),
            type_id,
            optional,
            is_method,
        }),
    }
}

impl Default for TypeNamespace<'_> {
    fn default() -> Self {
        Self::new()
    }
}
