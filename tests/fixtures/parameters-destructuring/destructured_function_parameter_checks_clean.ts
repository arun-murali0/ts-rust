interface Point {
    x: number;
    y: number;
}

function magnitude({ x, y }: Point): number {
    const sum: number = x + y;
    return sum;
}
