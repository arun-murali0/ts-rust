type Shape = { radius: number } | { size: number };

function positive(shape: Shape): number {
    if ("radius" in shape && shape.radius > 0) {
        return 1;
    }
    return 0;
}
