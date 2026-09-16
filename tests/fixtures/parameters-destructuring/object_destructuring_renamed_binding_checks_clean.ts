interface Point {
    x: number;
}

function readX(point: Point): number {
    const { x: horizontal } = point;
    return horizontal;
}
