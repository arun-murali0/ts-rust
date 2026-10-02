// Expect exactly one error: T is "a" from the first argument, and 1 is not "a".
function pair<T extends string | number>(a: T, b: T): T {
    return a;
}

const r = pair("a", 1);
