/// Stable identifiers for every diagnostic ts-rust can currently emit.
///
/// This is ts-rust's own namespace ("TSR####"), not TypeScript's TS####
/// numbering. Even where a code describes a condition tsc also reports
/// (an argument arity mismatch, say), the number here is not intended to
/// align with or imply equivalence to any specific tsc diagnostic code --
/// tsc's numbering is Microsoft's own catalog, not something to mirror.
///
/// Adding a variant here does not by itself make a diagnostic comparable to
/// tsc's -- see bin/compare-tsc.rs's module doc comment for why message
/// text still isn't compared verbatim. What this enables: stable
/// programmatic identity for a diagnostic across runs (useful for an LSP's
/// Diagnostic.code, for suppression comments, and for compare-tsc.rs to
/// eventually upgrade from position+severity to position+code once a
/// TSR-code <-> TS-code mapping table exists separately, without ever
/// emitting tsc's own codes as ts-rust's own).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "wasm", derive(serde::Serialize, serde::Deserialize))]
pub enum DiagnosticCode {
    // 1000s -- assignability / type mismatch
    BinaryOperandTypeMismatch,
    ArgumentNotAssignable,
    ReturnTypeMismatch,
    DeclaredTypeMismatch,
    DestructuringPatternTypeMismatch,
    StaticFieldInitializerMismatch,
    TypeArgumentConstraintViolation,
    ExcessProperty,

    // 1100s -- calls
    NotCallable,
    ArgumentArityMismatch,

    // 1200s -- name / member resolution
    UnresolvedIdentifier,
    PropertyDoesNotExist,

    // 1300s -- destructuring
    ArrayDestructuringRequiresArray,

    // 1400s -- implicit any
    ImplicitAnyParameter,
    ImplicitAnyElement,
    ImplicitAnyThis,

    // 9000s -- not yet implemented (all currently emitted as warnings)
    UnimplementedCallExpressionKind,
    UnimplementedNewExpressionTarget,
    UntypedParameterSkipsArityCheck,
    UnimplementedComputedMemberKey,
    UnimplementedOptionalChainLink,
    UnimplementedExpressionKind,
    UnimplementedStaticAccessor,
    UnimplementedClassShape,
    UnimplementedDestructuringKey,
    UnimplementedRestDestructuring,
    UnimplementedStatementKind,
    UnresolvableTypeAnnotation,
    UnresolvableDestructuringTypeAnnotation,
}

impl DiagnosticCode {
    pub fn as_str(self) -> &'static str {
        use DiagnosticCode::*;
        match self {
            BinaryOperandTypeMismatch => "TSR1001",
            ArgumentNotAssignable => "TSR1002",
            ReturnTypeMismatch => "TSR1003",
            DeclaredTypeMismatch => "TSR1004",
            DestructuringPatternTypeMismatch => "TSR1005",
            StaticFieldInitializerMismatch => "TSR1006",
            TypeArgumentConstraintViolation => "TSR1007",
            ExcessProperty => "TSR1008",

            NotCallable => "TSR1101",
            ArgumentArityMismatch => "TSR1102",

            UnresolvedIdentifier => "TSR1201",
            PropertyDoesNotExist => "TSR1202",

            ArrayDestructuringRequiresArray => "TSR1301",

            ImplicitAnyParameter => "TSR1401",
            ImplicitAnyElement => "TSR1402",
            ImplicitAnyThis => "TSR1403",

            UnimplementedCallExpressionKind => "TSR9001",
            UnimplementedNewExpressionTarget => "TSR9002",
            UntypedParameterSkipsArityCheck => "TSR9003",
            UnimplementedComputedMemberKey => "TSR9004",
            UnimplementedOptionalChainLink => "TSR9005",
            UnimplementedExpressionKind => "TSR9006",
            UnimplementedStaticAccessor => "TSR9007",
            UnimplementedClassShape => "TSR9008",
            UnimplementedDestructuringKey => "TSR9009",
            UnimplementedRestDestructuring => "TSR9010",
            UnimplementedStatementKind => "TSR9011",
            UnresolvableTypeAnnotation => "TSR9012",
            UnresolvableDestructuringTypeAnnotation => "TSR9013",
        }
    }
}

impl std::fmt::Display for DiagnosticCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
