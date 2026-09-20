interface Point {
    x: number;
}

function make(): Point {
    return { x: 1, y: 2 };
}

function take(p: Point): number {
    return p.x;
}

take({ x: 1, z: 3 });
