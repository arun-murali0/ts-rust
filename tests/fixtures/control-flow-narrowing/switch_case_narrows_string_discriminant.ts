type Shape = "circle" | "square" | "triangle";

function sides(shape: Shape): number {
    switch (shape) {
        case "circle": {
            const s: "circle" = shape;
            return 0;
        }
        case "square": {
            const s: "square" = shape;
            return 4;
        }
        default: {
            const s: "triangle" = shape;
            return 3;
        }
    }
}
