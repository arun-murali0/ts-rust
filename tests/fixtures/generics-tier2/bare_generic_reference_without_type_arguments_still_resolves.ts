interface Box<T> {
    value: T;
}

// A bare reference with no explicit type argument still resolves to Box's
// generic shape (T left as an unresolved placeholder), matching this
// checker's existing lenient treatment of anything it cannot fully pin down,
// rather than becoming a hard error.
function getValue(box: Box) {
    return box.value;
}
