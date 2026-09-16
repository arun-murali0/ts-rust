function nameOrDefault(name: string | null): string | number {
    const result: string | number = name || 0;
    return result;
}
