function lengthOf(read: () => string | null): number {
    return read().length;
}
