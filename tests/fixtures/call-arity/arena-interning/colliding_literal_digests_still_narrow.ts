// The type arena keys its intern table on a 64-bit digest, and the string literals
// "variant19" and "variant92" share one. The later literal used to miss the table, so
// `value.kind === "variant92"` could not find the member whose `kind` is "variant92"
// (the two copies of the literal had different ids) and `value` narrowed to never.
//
// Three variants, not two: once only one member is left, narrowing leaves it alone and
// the literal is never compared, so the collision would go unseen.
interface Variant19 {
    kind: "variant19";
    payload19: number;
}

interface Variant5 {
    kind: "variant5";
    payload5: number;
}

interface Variant92 {
    kind: "variant92";
    payload92: number;
}

type Triple = Variant19 | Variant5 | Variant92;

function read(value: Triple): number {
    if (value.kind === "variant19") {
        return value.payload19;
    } else if (value.kind === "variant92") {
        return value.payload92;
    }
    return value.payload5;
}
