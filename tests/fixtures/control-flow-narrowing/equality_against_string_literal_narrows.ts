type Shape = "circle" | "square";

function describe(shape: Shape): string {
    if (shape === "circle") {
        const s: "circle" = shape;
        return s;
    }
    const s: "square" = shape;
    return s;
}
