function describe(x: string | number): string {
    if (typeof x === "string") {
        return x;
    }
    return "not a string";
}
