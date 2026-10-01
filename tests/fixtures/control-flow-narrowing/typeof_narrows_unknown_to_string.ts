// Expect: no diagnostics (same as tsc).
// Gap if reported: typeof narrowing on a non-union (unknown) collapses to never.
function describe(x: unknown): string {
    if (typeof x === "string") {
        return x;
    }
    return "";
}
