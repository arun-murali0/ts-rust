function f(x: unknown): number {
    if (x === "a") {
        const s: "a" = x;
        return 1;
    }
    return 0;
}
