// Expect exactly one error: `size` does not exist on the radius member.
type Shape = { radius: number } | { size: number };

function measure(shape: Shape): number {
    if ("radius" in shape) {
        return shape.size;
    }
    return shape.size;
}
