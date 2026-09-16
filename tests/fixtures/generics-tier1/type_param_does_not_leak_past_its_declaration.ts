function identity<T>(value: T): T {
    return value;
}

// "T" was never declared at the top level; it only existed while checking
// identity's own signature/body. If it correctly doesn't leak, this annotation is
// unresolvable (a warning). If it incorrectly leaked, "5" would be checked against
// identity's own GenericParameter placeholder and fail as a type mismatch (an
// error). Either way exactly one diagnostic is produced, but which one tells us
// whether the scoping actually worked.
const leaked: T = 5;
