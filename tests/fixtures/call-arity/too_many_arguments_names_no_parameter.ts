function add(a: number, b: number): number {
    return a + b;
}

// Too many arguments has no single parameter to blame, so none is named.
add(1, 2, 3);
