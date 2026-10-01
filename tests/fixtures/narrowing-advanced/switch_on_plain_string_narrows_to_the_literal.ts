// A plain string discriminant, not a union: each case narrows it to its literal(s).
function pick(x: string): number {
    switch (x) {
        case "a": {
            const a: "a" = x;
            return 1;
        }
        case "b":
        case "c": {
            const bc: "b" | "c" = x;
            return 2;
        }
        default:
            return 0;
    }
}
