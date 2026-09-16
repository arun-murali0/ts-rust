interface Point {
    x: number;
    y: number;
}

function describe(point: Point): number {
    const { z } = point;
    return z;
}
