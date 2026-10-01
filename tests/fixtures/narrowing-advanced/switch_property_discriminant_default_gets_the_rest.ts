type Shape =
    | { kind: "circle"; radius: number }
    | { kind: "square"; size: number }
    | { kind: "triangle"; base: number };

function measure(shape: Shape): number {
    switch (shape.kind) {
        case "circle":
            return shape.radius;
        case "square":
            return shape.size;
        default:
            return shape.base;
    }
}
