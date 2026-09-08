function label(value: string | null | undefined): string {
    if (value) {
        return value;
    }
    return "empty";
}
