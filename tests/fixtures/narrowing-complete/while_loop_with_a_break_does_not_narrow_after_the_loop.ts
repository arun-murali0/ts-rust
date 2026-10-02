// Expect exactly one error: the break lets the loop end with x still a string, so x
// is string | null here and not assignable to null.
function f(x: string | null): number {
    while (x !== null) {
        break;
    }
    const done: null = x;
    return 0;
}
