function f(x: number | null | undefined): number {
    // `=== null` only narrows out null; undefined can still be here, so
    // returning x directly should still be a type mismatch.
    if (x === null) {
        return 0;
    }
    return x;
}
