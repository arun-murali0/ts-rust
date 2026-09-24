interface Box<T extends string> {
    value: T;
}

type Named<T extends { name: string }> = { item: T };

// Each type argument is assignable to its constraint, so nothing is reported.
const a: Box<string> = { value: "x" };
const c: Named<{ name: string; age: number }> = { item: { name: "n", age: 1 } };

// A type parameter used as the argument is left alone: it is checked where it
// is instantiated, not here.
function pass<T extends string>(box: Box<T>): T {
    return box.value;
}
