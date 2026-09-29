type Shape = "circle" | "square";

function describe(shape: Shape): string {
    switch (shape) {
        case "circle": {
            break;
        }
        case "square": {
            const s: "circle" = shape;
            return s;
        }
    }
    return "";
}
