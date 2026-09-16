// `typeof null` is "object", so the true branch keeps null and returning value
// where a Shape is expected is a genuine error.
type Shape = { kind: string };

function kindOf(value: Shape | null): Shape {
    if (typeof value === "object") {
        return value;
    }
    return { kind: "none" };
}
