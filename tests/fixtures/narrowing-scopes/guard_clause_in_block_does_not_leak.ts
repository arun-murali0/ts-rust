function f(x: number | null): number {
    {
        if (x === null) {
            return 0;
        }
    }
    // A bare block is its own scope, the same as an if-branch's.
    const y: number = x;
    return y;
}
