interface Pair<A, B> {
    first: A;
    second: B;
}

// Pair declares two type parameters, so one type argument is not enough.
const p: Pair<number> = { first: 1, second: 2 };
