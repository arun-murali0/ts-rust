function callOrReturn(value: (() => string) | string): string {
    if (typeof value === "function") {
        return value();
    }
    return value;
}
