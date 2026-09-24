interface Point {
    x: number;
    y: number;
}

// Point declares no type parameters, so it cannot be given type arguments.
const p: Point<number> = { x: 1, y: 2 };
