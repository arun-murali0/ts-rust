interface Overloaded {
    pick(value: string): string;
    pick(value: number): number;
}

// Overloads have no representation here, so the interface stays unresolved and
// nothing is reported: the same result tsc gives for this valid code.
function use(o: Overloaded): string {
    return o.pick("x");
}
