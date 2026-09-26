function f(a: any): string {
    // Verified against real tsc: unlike `+`, `any - 1` actually infers
    // `number`, not `any` -- only `+` has both a string and a number
    // overload, which is what makes `any` ambiguous there specifically.
    // Assigning to a `string` target makes this observable: since the
    // result really is `number`, this line is a genuine type mismatch.
    const result: string = a - 1;
    return result;
}
