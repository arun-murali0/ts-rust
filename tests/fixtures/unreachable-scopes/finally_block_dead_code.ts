function f(): number {
    try {
        return 1;
    } finally {
        const a = 1;
        throw new Error("x");
        const b = 2;
    }
}
