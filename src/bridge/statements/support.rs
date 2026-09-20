use oxc_ast::ast::{BindingPattern, FormalParameters, Statement, TSSignature, TSType, TSTypeName};
use oxc_semantic::Scoping;
use oxc_span::{GetSpan, Span};

use crate::namespace::TypeNamespace;

use super::super::context::CheckContext;
use super::super::narrow::resolve_symbol_id;

pub(super) fn push_unsupported(stmt: &Statement, ctx: &mut CheckContext<'_, '_>) {
    let kind = stmt_kind_name(stmt);
    tracing::trace!(kind, "unsupported statement kind");
    ctx.warning(
        crate::diagnostic_messages::messages::unimplemented_statement_kind(kind),
        stmt.span(),
    );
}

fn stmt_kind_name(stmt: &Statement) -> &'static str {
    match stmt {
        Statement::ImportDeclaration(_) => "ImportDeclaration",
        _ => "Other",
    }
}

// Used by control_flow.rs to decide whether an if branch's narrowing should
// survive past the whole if statement. `if (x === null) return; ... x.prop`
// depends on this: since the consequent branch always exits, only the else
// branch's narrowing (x is non-null) can reach code after the if, and that
// narrowing should carry forward rather than be discarded at the closing brace.
pub(crate) fn statement_always_exits(stmt: &Statement) -> bool {
    match stmt {
        Statement::ReturnStatement(_) | Statement::ThrowStatement(_) => true,
        Statement::BlockStatement(block) => block.body.last().is_some_and(statement_always_exits),
        Statement::IfStatement(if_stmt) => match &if_stmt.alternate {
            Some(alternate) => {
                statement_always_exits(&if_stmt.consequent) && statement_always_exits(alternate)
            }
            None => false,
        },
        _ => false,
    }
}

// Reports every parameter with no type annotation and no default value as an
// implicit `any`, the same condition tsc reports under noImplicitAny. Callers
// decide *where* this applies: it is only called for parameter lists that can
// never receive a contextual type (function declarations, class constructors
// and methods, and arrow or function expressions used as the initializer of an
// unannotated variable). An arrow passed as a call argument is deliberately not
// reported, since tsc would type its parameters from the callee's signature and
// this checker does not do contextual typing yet.
//
// A destructured parameter, a parameter with a default (`x = 1`), and a rest
// parameter are skipped: tsc infers from the default, and reports the other two
// under different diagnostics.
pub(super) fn report_implicit_any_params(
    params: &FormalParameters,
    ctx: &mut CheckContext<'_, '_>,
) {
    for param in &params.items {
        if param.type_annotation.is_some() {
            continue;
        }
        let BindingPattern::BindingIdentifier(id) = &param.pattern else {
            continue;
        };
        ctx.error(
            crate::diagnostic_messages::messages::parameter_implicitly_any(&id.name),
            id.span,
        );
    }
}

// Finds the first type name in an annotation that genuinely does not exist, as
// opposed to one that exists but this checker cannot represent. A name counts as
// missing only if oxc's own scope analysis resolved it to no declaration at all
// (which also correctly covers imports, exported declarations and type
// parameters that have gone out of scope), it is not in this checker's own
// namespace, and it is not a lib.d.ts global. Anything else that fails to
// resolve is an unsupported construct, and stays a warning.
pub(super) fn find_unresolved_type_name(
    ty: &TSType,
    scoping: &Scoping,
    namespace: &TypeNamespace,
) -> Option<(String, Span)> {
    match ty {
        TSType::TSTypeReference(reference) => {
            let TSTypeName::IdentifierReference(id) = &reference.type_name else {
                return None;
            };
            let is_missing = resolve_symbol_id(id, scoping).is_none()
                && !namespace.contains(&id.name)
                && !is_global_lib_type(&id.name);
            is_missing.then(|| (id.name.to_string(), id.span))
        }
        TSType::TSArrayType(array) => {
            find_unresolved_type_name(&array.element_type, scoping, namespace)
        }
        TSType::TSUnionType(union) => union
            .types
            .iter()
            .find_map(|member| find_unresolved_type_name(member, scoping, namespace)),
        TSType::TSTypeLiteral(literal) => literal.members.iter().find_map(|member| {
            let TSSignature::TSPropertySignature(property) = member else {
                return None;
            };
            let annotation = property.type_annotation.as_ref()?;
            find_unresolved_type_name(&annotation.type_annotation, scoping, namespace)
        }),
        TSType::TSFunctionType(func) => func
            .params
            .items
            .iter()
            .find_map(|param| {
                let annotation = param.type_annotation.as_ref()?;
                find_unresolved_type_name(&annotation.type_annotation, scoping, namespace)
            })
            .or_else(|| {
                find_unresolved_type_name(&func.return_type.type_annotation, scoping, namespace)
            }),
        _ => None,
    }
}

// Names that are always in scope in TypeScript without any declaration in the
// file, because lib.d.ts provides them. ts-rust does not model these, so a
// reference to one is an unsupported type, not a missing name.
fn is_global_lib_type(name: &str) -> bool {
    matches!(
        name,
        "Array"
            | "ReadonlyArray"
            | "Promise"
            | "PromiseLike"
            | "Record"
            | "Partial"
            | "Required"
            | "Readonly"
            | "Pick"
            | "Omit"
            | "Exclude"
            | "Extract"
            | "NonNullable"
            | "ReturnType"
            | "Parameters"
            | "ConstructorParameters"
            | "InstanceType"
            | "Awaited"
            | "Uppercase"
            | "Lowercase"
            | "Capitalize"
            | "Uncapitalize"
            | "ThisType"
            | "Map"
            | "Set"
            | "WeakMap"
            | "WeakSet"
            | "Date"
            | "RegExp"
            | "Error"
            | "Function"
            | "Object"
            | "String"
            | "Number"
            | "Boolean"
            | "Symbol"
            | "BigInt"
            | "Iterable"
            | "Iterator"
            | "IterableIterator"
            | "AsyncIterable"
            | "AsyncIterator"
            | "ArrayLike"
    )
}
