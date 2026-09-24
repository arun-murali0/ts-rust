type Pair<A, B> = {
    first: A;
    second: B;
};

// second must be string, not number.
const p: Pair<number, string> = { first: 1, second: 2 };
