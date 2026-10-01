// Expect: no diagnostics (same as tsc).
// Gap if reported: assigning inside `if (x !== null)` is checked against the
// narrowed type (string) instead of the declared type (string | null).
function reset(x: string | null): void {
    if (x !== null) {
        x = null;
    }
}
