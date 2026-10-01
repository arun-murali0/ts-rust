type Shape =
    | { kind: "circle"; radius: number }
    | { kind: "square"; size: number };

function area(shape: Shape): number {
    switch (shape.kind) {
        case "circle":
            return shape.radius * shape.radius * 3;
        case "square":
            return shape.size * shape.size;
        default:
            return 0;
    }
}
