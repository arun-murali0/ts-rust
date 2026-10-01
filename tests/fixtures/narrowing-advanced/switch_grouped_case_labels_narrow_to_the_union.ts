// Empty case labels fall into the next body, so the body is narrowed to either.
type Kind = "a" | "b" | "c";

function label(k: Kind): string {
    switch (k) {
        case "a":
        case "b": {
            const narrowed: "a" | "b" = k;
            return narrowed;
        }
        default: {
            const rest: "c" = k;
            return rest;
        }
    }
}
