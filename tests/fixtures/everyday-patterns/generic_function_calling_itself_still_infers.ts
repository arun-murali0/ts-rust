function wrap<T>(value: T, depth: number): T[] {
    if (depth <= 0) {
        return [value];
    }
    return wrap(value, depth - 1);
}
