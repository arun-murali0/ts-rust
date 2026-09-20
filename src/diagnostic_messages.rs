use crate::diagnostic_codes::DiagnosticCode;

/// A diagnostic's code and text, produced together by exactly one
/// constructor per kind of diagnostic (see the `messages` module below).
///
/// This exists so a call site can never pass a `DiagnosticCode` and a
/// message string that don't actually correspond to each other -- there is
/// no way to construct one of these except through a function that already
/// knows which code goes with which text. `ctx.error()`/`ctx.warning()`
/// take one of these instead of a `(code, message)` pair.
pub struct DiagnosticMessage {
    pub code: DiagnosticCode,
    pub text: String,
}

impl DiagnosticMessage {
    fn new(code: DiagnosticCode, text: impl Into<String>) -> Self {
        Self {
            code,
            text: text.into(),
        }
    }
}

/// One function per diagnostic kind. A `DiagnosticCode` that has more than
/// one distinct phrasing (e.g. "not callable" vs "not a constructor") gets
/// more than one function here, still sharing the one code -- what this
/// module guarantees is that code and text are chosen together, in one
/// place, not that every code maps to exactly one string.
pub mod messages {
    use super::{DiagnosticCode, DiagnosticMessage};

    pub fn binary_operand_type_mismatch(operator: &str) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::BinaryOperandTypeMismatch,
            format!("Operator '{operator}' cannot be applied to these types."),
        )
    }

    pub fn argument_not_assignable() -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::ArgumentNotAssignable,
            "Argument type is not assignable to parameter type.",
        )
    }

    pub fn return_type_mismatch() -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::ReturnTypeMismatch,
            "Return type does not match the function's declared return type.",
        )
    }

    pub fn declared_type_mismatch(name: &str) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::DeclaredTypeMismatch,
            format!("Type mismatch: value is not assignable to declared type of '{name}'."),
        )
    }

    pub fn destructuring_pattern_type_mismatch() -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::DestructuringPatternTypeMismatch,
            "Type mismatch: value is not assignable to the destructuring pattern's declared type.",
        )
    }

    pub fn static_field_initializer_mismatch() -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::StaticFieldInitializerMismatch,
            "Static field initializer is not assignable to its declared type.",
        )
    }

    pub fn type_argument_constraint_violation(name: &str) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::TypeArgumentConstraintViolation,
            format!("Type does not satisfy the constraint of type parameter '{name}'."),
        )
    }

    pub fn excess_property(name: &str) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::ExcessProperty,
            format!(
                "Object literal may only specify known properties, and '{name}' does not exist in the target type."
            ),
        )
    }

    pub fn parameter_implicitly_any(name: &str) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::ImplicitAnyParameter,
            format!("Parameter '{name}' implicitly has an 'any' type."),
        )
    }

    pub fn not_callable(name: &str) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::NotCallable,
            format!("'{name}' is not callable."),
        )
    }

    pub fn not_a_constructor(name: &str) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::NotCallable,
            format!("'{name}' is not a constructor."),
        )
    }

    pub fn argument_arity_exact(required: usize, got: usize) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::ArgumentArityMismatch,
            format!("Expected {required} argument(s), but got {got}."),
        )
    }

    pub fn argument_arity_at_least(required: usize, got: usize) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::ArgumentArityMismatch,
            format!("Expected at least {required} argument(s), but got {got}."),
        )
    }

    pub fn argument_arity_range(required: usize, max: usize, got: usize) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::ArgumentArityMismatch,
            format!("Expected {required}-{max} argument(s), but got {got}."),
        )
    }

    pub fn unresolved_identifier(name: &str) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::UnresolvedIdentifier,
            format!("Cannot find name '{name}'."),
        )
    }

    pub fn property_does_not_exist(property_name: &str) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::PropertyDoesNotExist,
            format!("Property '{property_name}' does not exist on this type."),
        )
    }

    pub fn array_destructuring_requires_array() -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::ArrayDestructuringRequiresArray,
            "Array destructuring requires an array type.",
        )
    }

    pub fn unimplemented_call_expression_kind() -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::UnimplementedCallExpressionKind,
            "This kind of call expression is not yet checked by ts-rust.",
        )
    }

    pub fn unimplemented_new_expression_target() -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::UnimplementedNewExpressionTarget,
            "`new` on anything other than a plain name is not yet checked by ts-rust.",
        )
    }

    pub fn untyped_parameter_skips_arity_check(name: &str) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::UntypedParameterSkipsArityCheck,
            format!(
                "'{name}' has an untyped parameter, so ts-rust can't check this call's arity yet."
            ),
        )
    }

    pub fn untyped_constructor_parameter_skips_arity_check(name: &str) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::UntypedParameterSkipsArityCheck,
            format!(
                "'{name}' has a constructor with an untyped parameter, so ts-rust can't \
                 check arity for `new {name}(...)` yet."
            ),
        )
    }

    pub fn unimplemented_computed_member_key() -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::UnimplementedComputedMemberKey,
            "Computed member access with a non-literal key is not yet checked by ts-rust.",
        )
    }

    pub fn unimplemented_optional_chain_link() -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::UnimplementedOptionalChainLink,
            "This kind of optional-chain link is not yet checked by ts-rust.",
        )
    }

    pub fn unimplemented_expression_kind() -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::UnimplementedExpressionKind,
            "This expression kind is not yet checked by ts-rust.",
        )
    }

    pub fn unimplemented_static_accessor() -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::UnimplementedStaticAccessor,
            "Static getter/setter is not yet checked by ts-rust.",
        )
    }

    pub fn unimplemented_class_shape(name: &str) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::UnimplementedClassShape,
            format!(
                "Class '{name}' uses a shape not yet checked by ts-rust: an unresolvable \
                 field or method, or a superclass that isn't a plain class name."
            ),
        )
    }

    pub fn unimplemented_destructuring_key() -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::UnimplementedDestructuringKey,
            "Computed or non-identifier destructuring keys are not yet checked by ts-rust.",
        )
    }

    pub fn unimplemented_rest_destructuring() -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::UnimplementedRestDestructuring,
            "Rest destructuring (`...rest`) does not yet compute a precise type; \
             the binding is not checked by ts-rust.",
        )
    }

    pub fn unimplemented_statement_kind(kind: &str) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::UnimplementedStatementKind,
            format!("This statement kind is not yet checked by ts-rust: {kind}."),
        )
    }

    pub fn unresolvable_type_annotation(name: &str) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::UnresolvableTypeAnnotation,
            format!(
                "Type annotation for '{name}' could not be resolved (unknown name, \
                 or its definition isn't fully understood by ts-rust yet)."
            ),
        )
    }

    pub fn unresolvable_destructuring_type_annotation() -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::UnresolvableDestructuringTypeAnnotation,
            "Type annotation for this destructuring pattern could not be resolved.",
        )
    }
}
