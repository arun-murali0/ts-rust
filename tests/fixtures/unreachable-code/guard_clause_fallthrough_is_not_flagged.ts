function f(x: number | null): number {
    // Reachable through the false branch of the `if`, so this must not be
    // flagged: only the true branch of this `if` exits.
    if (x === null) {
        return 0;
    }
    return x;
}
