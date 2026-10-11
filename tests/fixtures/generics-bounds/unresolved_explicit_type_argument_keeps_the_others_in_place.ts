// tsc reports the unknown name `Nope`, and so does this checker. The point of the test is
// that nothing else is reported: the unknown first argument does not shift the second one,
// which still lands on `B`, so `two<Nope, string>(1, "x")` is a string.
function two<A, B>(a: A, b: B): B {
    return b;
}

const r: string = two<Nope, string>(1, "x");
