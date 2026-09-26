function f(a: any): number {
    // any + 1 is `any` in real TypeScript, not `string`: `any` is assignable to
    // both string and number, and the string check must not win just because
    // it happens to be tried first. Assigning to a `number` target makes the
    // difference observable: if this actually inferred `string`, the line
    // below would be a type mismatch.
    const result: number = a + 1;
    return result;
}
