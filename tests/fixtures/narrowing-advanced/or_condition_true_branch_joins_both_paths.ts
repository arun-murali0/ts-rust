function f(x: string | number | null): string | number {
    if (x === null || typeof x === "string") {
        const v: string | null = x;
        return 1;
    }
    return x;
}
