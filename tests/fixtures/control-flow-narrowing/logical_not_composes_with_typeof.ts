function stringify(x: string | number): string {
    if (!(typeof x === "number")) {
        return x.toUpperCase();
    }
    return x.toFixed(2);
}
