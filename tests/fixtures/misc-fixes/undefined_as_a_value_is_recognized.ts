function f(x: number | undefined): number {
    if (x === undefined) {
        return 0;
    }
    return x;
}

// undefined used directly as a value, not just compared against, must not be
// "Cannot find name 'undefined'".
const y: number | undefined = undefined;
