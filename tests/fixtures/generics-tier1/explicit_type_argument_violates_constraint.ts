interface HasLength {
    length: number;
}

function logLength<T extends HasLength>(value: T): number {
    return value.length;
}

// number has no length property, so binding T = number explicitly must still
// be checked against the constraint, exactly as an inferred binding would be.
logLength<number>(5);
