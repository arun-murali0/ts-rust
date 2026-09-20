declare function compute(): number;

enum Weird {
    A = compute(),
}

function value(): number {
    return Weird.A;
}
