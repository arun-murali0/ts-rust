// An omitted trailing type argument falls back to its declared default.
interface Pair<A, B = string> {
    first: A;
    second: B;
}

const ok: Pair<number> = { first: 1, second: "x" };
const bad: Pair<number> = { first: 1, second: 2 };
