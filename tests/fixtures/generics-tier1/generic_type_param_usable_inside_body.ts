function wrapInArray<T>(value: T): T[] {
    const box: T[] = [value];
    return box;
}

const numbers: number[] = wrapInArray(5);
