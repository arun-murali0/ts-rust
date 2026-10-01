// After `a && b` fails, either one may still be null: nothing is over-narrowed.
function either(a: string | null, b: number | null): number {
    if (a !== null && b !== null) {
        return 1;
    }
    const maybeA: string | null = a;
    const maybeB: number | null = b;
    return 0;
}
