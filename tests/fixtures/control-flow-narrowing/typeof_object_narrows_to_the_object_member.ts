type Shape = { kind: string };

function kindOf(value: Shape | string): string {
    if (typeof value === "object") {
        return value.kind;
    }
    return value;
}
