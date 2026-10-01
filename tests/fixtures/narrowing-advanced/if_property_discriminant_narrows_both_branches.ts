type Shape =
    | { kind: "circle"; radius: number }
    | { kind: "square"; size: number };

function measure(shape: Shape): number {
    if (shape.kind === "circle") {
        return shape.radius;
    }
    return shape.size;
}
