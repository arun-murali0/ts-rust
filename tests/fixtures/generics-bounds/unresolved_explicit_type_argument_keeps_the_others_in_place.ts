// tsc reports the unknown name `Nope`. This checker does not report unresolved type
// names, so the point here is that the second argument still lands on `B`.
function two<A, B>(a: A, b: B): B {
    return b;
}

const r: string = two<Nope, string>(1, "x");
