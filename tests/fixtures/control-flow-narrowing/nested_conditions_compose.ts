function describe(value: string | number | null): string {
    if (typeof value !== "number") {
        if (value !== null) {
            return value;
        }
    }
    return "other";
}
