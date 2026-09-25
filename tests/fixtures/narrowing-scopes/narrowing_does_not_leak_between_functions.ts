function f(x: number | null): number {
    if (x === null) {
        return 0;
    }
    return x;
}

function g(x: number | null): number {
    // A different function, a different binding for x. If narrowing from f
    // leaked, this would wrongly be accepted with no guard clause of its own.
    return x;
}
