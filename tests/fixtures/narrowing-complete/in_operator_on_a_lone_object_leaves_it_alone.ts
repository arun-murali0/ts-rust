function f(o: { a: number }): number {
    if ("b" in o) {
        return o.a;
    }
    return o.a;
}
