function f(x: string | null): string {
    if (x !== null) {
        const inner: string = x;
    } else {
        return "none";
    }
    const after: string = x;
    return after;
}
