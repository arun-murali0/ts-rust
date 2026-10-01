type Kind = "a" | "b" | "c";

function describe(k: Kind): string {
    switch (k) {
        case "a":
            return "first";
        default: {
            const rest: "b" | "c" = k;
            return rest;
        }
    }
}
