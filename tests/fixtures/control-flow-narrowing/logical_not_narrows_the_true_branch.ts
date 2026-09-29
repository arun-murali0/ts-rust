function describe(x: string | null): string {
    if (!x) {
        return "empty";
    }
    return x.toUpperCase();
}
