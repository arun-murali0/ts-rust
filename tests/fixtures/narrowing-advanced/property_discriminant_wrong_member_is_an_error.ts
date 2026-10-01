// Expect exactly one error: `size` does not exist on the circle member.
type Shape =
    | { kind: "circle"; radius: number }
    | { kind: "square"; size: number };

function measure(shape: Shape): number {
    if (shape.kind === "circle") {
        return shape.size;
    }
    return shape.size;
}
