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
    use crate::arena::{TypeArena, TypeId};
    use crate::type_display::{display_source_type, display_type};

    pub fn binary_operand_type_mismatch(
        arena: &TypeArena,
        operator: &str,
        left: TypeId,
        right: TypeId,
    ) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::BinaryOperandTypeMismatch,
            format!(
                "Operator '{operator}' cannot be applied to types '{}' and '{}'.",
                display_type(arena, left),
                display_type(arena, right)
            ),
        )
    }

    // tsc reports an arithmetic operator (`-`, `*`, ...) on a bad operand as one
    // diagnostic per bad side, not the "cannot be applied to types" of `+`.
    pub fn arithmetic_left_operand_invalid() -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::BinaryOperandTypeMismatch,
            "The left-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type.",
        )
    }

    pub fn arithmetic_right_operand_invalid() -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::BinaryOperandTypeMismatch,
            "The right-hand side of an arithmetic operation must be of type 'any', 'number', 'bigint' or an enum type.",
        )
    }

    pub fn argument_not_assignable(
        arena: &TypeArena,
        actual: TypeId,
        expected: TypeId,
    ) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::ArgumentNotAssignable,
            format!(
                "Argument of type '{}' is not assignable to parameter of type '{}'.",
                display_source_type(arena, actual, expected),
                display_type(arena, expected)
            ),
        )
    }

    pub fn return_type_mismatch(
        arena: &TypeArena,
        actual: TypeId,
        expected: TypeId,
    ) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::ReturnTypeMismatch,
            format!(
                "Type '{}' is not assignable to type '{}'.",
                display_source_type(arena, actual, expected),
                display_type(arena, expected)
            ),
        )
    }

    pub fn declared_type_mismatch(
        arena: &TypeArena,
        actual: TypeId,
        expected: TypeId,
    ) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::DeclaredTypeMismatch,
            format!(
                "Type '{}' is not assignable to type '{}'.",
                display_source_type(arena, actual, expected),
                display_type(arena, expected)
            ),
        )
    }

    pub fn destructuring_pattern_type_mismatch() -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::DestructuringPatternTypeMismatch,
            "Type mismatch: value is not assignable to the destructuring pattern's declared type.",
        )
    }

    pub fn static_field_initializer_mismatch(
        arena: &TypeArena,
        actual: TypeId,
        expected: TypeId,
    ) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::StaticFieldInitializerMismatch,
            format!(
                "Type '{}' is not assignable to type '{}'.",
                display_source_type(arena, actual, expected),
                display_type(arena, expected)
            ),
        )
    }

    // tsc's TS2344 names only the constraint, never the type parameter, so the message
    // carries no parameter name.
    pub fn type_argument_constraint_violation(
        arena: &TypeArena,
        actual: TypeId,
        constraint: TypeId,
    ) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::TypeArgumentConstraintViolation,
            format!(
                "Type '{}' does not satisfy the constraint '{}'.",
                display_type(arena, actual),
                display_type(arena, constraint)
            ),
        )
    }

    pub fn excess_property(arena: &TypeArena, name: &str, target: TypeId) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::ExcessProperty,
            format!(
                "Object literal may only specify known properties, and '{name}' does not exist in type '{}'.",
                display_type(arena, target)
            ),
        )
    }

    pub fn property_not_initialized(name: &str) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::PropertyNotInitialized,
            format!(
                "Property '{name}' has no initializer and is not definitely assigned in the constructor."
            ),
        )
    }

    pub fn parameter_implicitly_any(name: &str) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::ImplicitAnyParameter,
            format!("Parameter '{name}' implicitly has an 'any' type."),
        )
    }

    pub fn element_implicitly_any(
        arena: &TypeArena,
        key: TypeId,
        object: TypeId,
    ) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::ImplicitAnyElement,
            format!(
                "Element implicitly has an 'any' type because expression of type '{}' can't be used to index type '{}'.",
                display_type(arena, key),
                display_type(arena, object)
            ),
        )
    }

    pub fn this_implicitly_any() -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::ImplicitAnyThis,
            "'this' implicitly has type 'any' because it does not have a type annotation.",
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

    // tsc's TS2554 always says "arguments", even for one, and never names the
    // missing parameter, so these match it word for word.
    pub fn argument_arity_exact(required: usize, got: usize) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::ArgumentArityMismatch,
            format!("Expected {required} arguments, but got {got}."),
        )
    }

    pub fn argument_arity_at_least(required: usize, got: usize) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::ArgumentArityMismatch,
            format!("Expected at least {required} arguments, but got {got}."),
        )
    }

    pub fn argument_arity_range(required: usize, max: usize, got: usize) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::ArgumentArityMismatch,
            format!("Expected {required}-{max} arguments, but got {got}."),
        )
    }

    pub fn unresolved_identifier(name: &str) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::UnresolvedIdentifier,
            format!("Cannot find name '{name}'."),
        )
    }

    pub fn property_does_not_exist(
        arena: &TypeArena,
        property_name: &str,
        object_type: TypeId,
    ) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::PropertyDoesNotExist,
            format!(
                "Property '{property_name}' does not exist on type '{}'.",
                display_type(arena, object_type)
            ),
        )
    }

    pub fn array_destructuring_requires_array(
        arena: &TypeArena,
        source: TypeId,
    ) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::ArrayDestructuringRequiresArray,
            format!(
                "Type '{}' must have a '[Symbol.iterator]()' method that returns an iterator.",
                display_type(arena, source)
            ),
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

    pub fn unresolvable_type_parameter_constraint(name: &str) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::UnresolvableTypeParameterConstraint,
            format!(
                "The constraint of type parameter '{name}' could not be resolved, so it is \
                 treated as unconstrained: it is not enforced at call sites and its members \
                 are not usable inside the function."
            ),
        )
    }

    // `required == total` is tsc's TS2314; a range (some trailing parameters have
    // defaults) is its TS2707. `display_name` is the generic's name with its
    // parameter list (`Box<T>`), because tsc prints it that way.
    pub fn type_argument_count_mismatch(
        display_name: &str,
        required: usize,
        total: usize,
    ) -> DiagnosticMessage {
        let text = if required == total {
            format!("Generic type '{display_name}' requires {total} type argument(s).")
        } else {
            format!(
                "Generic type '{display_name}' requires between {required} and {total} type arguments."
            )
        };
        DiagnosticMessage::new(DiagnosticCode::TypeArgumentCountMismatch, text)
    }

    pub fn duplicate_type_declaration(name: &str) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::DuplicateTypeDeclaration,
            format!("Duplicate identifier '{name}'."),
        )
    }

    pub fn type_is_not_generic(name: &str) -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::TypeIsNotGeneric,
            format!("Type '{name}' is not generic."),
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

    pub fn unreachable_code() -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::UnreachableCode,
            "Unreachable code detected.",
        )
    }

    pub fn unresolvable_destructuring_type_annotation() -> DiagnosticMessage {
        DiagnosticMessage::new(
            DiagnosticCode::UnresolvableDestructuringTypeAnnotation,
            "Type annotation for this destructuring pattern could not be resolved.",
        )
    }
}
