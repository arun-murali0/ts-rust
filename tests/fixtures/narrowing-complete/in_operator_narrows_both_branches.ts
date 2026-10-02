type Shape = { radius: number } | { size: number };

function measure(shape: Shape): number {
    if ("radius" in shape) {
        return shape.radius;
    }
    return shape.size;
}
