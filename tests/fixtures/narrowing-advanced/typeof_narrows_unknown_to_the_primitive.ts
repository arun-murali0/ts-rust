// Expect exactly one error: x is a string here, which is not a number.
function f(x: unknown): number {
    if (typeof x === "string") {
        const bad: number = x;
        return bad;
    }
    return 0;
}
