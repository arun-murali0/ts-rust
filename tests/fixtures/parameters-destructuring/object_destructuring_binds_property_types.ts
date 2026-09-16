interface Point {
    x: number;
    y: number;
}

function distanceFromOrigin(point: Point): number {
    const { x, y } = point;
    const total: number = x + y;
    return total;
}
