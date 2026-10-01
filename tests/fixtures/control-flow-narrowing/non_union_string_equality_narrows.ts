// Expect: no diagnostics (same as tsc).
// Gap if reported: narrowing a non-union `string` by === "a" collapses to never.
function describe(x: string): string {
    if (x === "a") {
        return x;
    }
    return x;
}
