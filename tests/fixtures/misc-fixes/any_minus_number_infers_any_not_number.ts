function f(a: any): string {
    // Same issue as addition, less visible: `any` is also assignable to
    // `number`, so `any - 1` must stay `any`, not silently become `number`.
    // Assigning to a `string` target makes the difference observable: if this
    // actually inferred `number`, the line below would be a type mismatch.
    const result: string = a - 1;
    return result;
}
