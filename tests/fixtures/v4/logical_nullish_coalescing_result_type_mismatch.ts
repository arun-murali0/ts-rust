function readOrDefault(value: string | null): number {
    const result: number = value ?? "fallback";
    return result;
}
