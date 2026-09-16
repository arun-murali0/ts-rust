enum Weird {
    A = 1 << 0,
    B,
}

function value(): string {
    return Weird.A;
}
