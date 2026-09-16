function greet(name: string | null): string {
    if (name === null) {
        return "anonymous";
    }
    return name;
}
