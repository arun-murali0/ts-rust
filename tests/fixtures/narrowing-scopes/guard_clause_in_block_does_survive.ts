function f(x: number | null): number {
    {
        if (x === null) {
            return 0;
        }
    }
    // Verified against real tsc: unlike while/for/switch/a function body, a
    // bare `{ ... }` is not a control-flow construct at all, so it has no
    // effect on narrowing. x is correctly still narrowed to non-null number
    // here, the same as it would be with no block at all.
    return x;
}
