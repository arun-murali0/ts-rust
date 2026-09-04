// Known gap, tracked in docs/ROADMAP.md: real TypeScript recognizes that
// the `if` branch always returns, so `name` is narrowed to `string` for
// the rest of the function. ts-rust's narrowing is scoped strictly to
// inside the branch that earned it and reverts once the `if` ends, so
// `name` here is still `string | null`, and this incorrectly flags.
function greet(name: string | null): string {
    if (name === null) {
        return "anonymous";
    }
    return name;
}
