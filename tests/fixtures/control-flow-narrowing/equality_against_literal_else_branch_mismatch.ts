type Shape = "circle" | "square";

function describe(shape: Shape): string {
    if (shape === "circle") {
        return shape;
    }
    const s: "circle" = shape;
    return s;
}
