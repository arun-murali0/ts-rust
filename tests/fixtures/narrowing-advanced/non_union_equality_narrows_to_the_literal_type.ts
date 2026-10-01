// Expect exactly one error: x is "a" here, which is not a number. A lone string
// narrowed to never would be assignable to anything and report nothing.
function f(x: string): number {
    if (x === "a") {
        const bad: number = x;
        return bad;
    }
    return 0;
}
