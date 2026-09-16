// "symbol" matches no type ts-rust models. The true branch must keep value as
// it was rather than collapsing to never and reporting the return as an error.
function describe(value: string): string {
    if (typeof value === "symbol") {
        return value;
    }
    return value;
}
