function f(x: number | null | undefined): number {
    // `== null` is JavaScript's specific loose-equality-with-null quirk: it
    // matches both null and undefined, unlike `=== null`, which only matches
    // null. This must narrow both out, leaving only number.
    if (x == null) {
        return 0;
    }
    return x;
}
