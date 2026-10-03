function f(x: number): number {
    if (x < 0) {
        throw new Error("neg");
    }
    return x;
}
