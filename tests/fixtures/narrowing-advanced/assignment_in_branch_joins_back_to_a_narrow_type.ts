// The default-value pattern: after the if, x is a string on both paths.
function withDefault(x: string | null): string {
    if (x === null) {
        x = "default";
    }
    return x;
}
