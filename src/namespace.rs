use oxc_ast::ast::{
    Class, ClassElement, Expression, MethodDefinitionKind, PropertyKey, TSInterfaceDeclaration,
    TSType,
};

use crate::arena::{TypeArena, TypeId};
use crate::fxhash::FxHashMap;
use crate::type_annotation::{
    resolve_function_params, resolve_object_members, resolve_ts_type, resolve_type_annotation,
};
use crate::types::{ObjectType, PropertyEntry, Type};

#[derive(Clone, Copy)]
enum DeclKind<'a> {
    TypeAlias(&'a TSType<'a>),
    Interface(&'a TSInterfaceDeclaration<'a>),
    Class(&'a Class<'a>),

    Resolved,
}

pub struct TypeParamScope<'a>(Vec<(String, Option<TypeEntry<'a>>)>);

struct TypeEntry<'a> {
    kind: DeclKind<'a>,
    resolved: Option<TypeId>,

    resolving: bool,
}

pub struct TypeNamespace<'a> {
    entries: FxHashMap<String, TypeEntry<'a>>,

    type_param_cache: FxHashMap<usize, TypeId>,
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
        }
    }

    pub fn insert_type_alias(&mut self, name: &str, body: &'a TSType<'a>) {
        self.entries.insert(
            name.to_string(),
            TypeEntry {
                kind: DeclKind::TypeAlias(body),
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

    pub fn push_type_params(
        &mut self,
        arena: &mut TypeArena,
        func: &oxc_ast::ast::Function,
    ) -> TypeParamScope<'a> {
        let Some(decl) = &func.type_parameters else {
            return TypeParamScope(Vec::new());
        };

        let mut saved = Vec::with_capacity(decl.params.len());
        for param in &decl.params {
            let name = param.name.name.to_string();
            let cache_key = std::ptr::addr_of!(*param) as usize;
            let type_id = *self
                .type_param_cache
                .entry(cache_key)
                .or_insert_with(|| arena.alloc(Type::GenericParameter(name.clone())));

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
        let resolved = match kind {
            DeclKind::TypeAlias(body) => resolve_ts_type(body, self, arena),
            DeclKind::Interface(decl) => resolve_object_members(&decl.body.body, self, arena),
            DeclKind::Class(class) => self.resolve_class(class, arena),
            DeclKind::Resolved => None,
        };

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
                    upsert_property(&mut properties, key.name.to_string(), method_type, false);
                }

                _ => {}
            }
        }

        properties.sort_by(|a, b| a.name.cmp(&b.name));
        Some(arena.alloc(Type::Object(ObjectType { properties })))
    }
}

fn upsert_property(
    properties: &mut Vec<PropertyEntry>,
    name: String,
    type_id: TypeId,
    optional: bool,
) {
    match properties.iter_mut().find(|p| p.name == name) {
        Some(existing) => {
            existing.type_id = type_id;
            existing.optional = optional;
        }
        None => properties.push(PropertyEntry {
            name,
            type_id,
            optional,
        }),
    }
}

impl Default for TypeNamespace<'_> {
    fn default() -> Self {
        Self::new()
    }
}
