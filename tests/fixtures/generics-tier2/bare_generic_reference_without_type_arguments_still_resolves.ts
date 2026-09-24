interface Box<T> {
  value: T;
}

// A bare reference to a generic type with no type arguments is an error in tsc
// (TS2314: Generic type 'Box<T>' requires 1 type argument(s)). It is reported
// once, and the parameter is typed as the error type so the body does not add a
// second, unrelated diagnostic.
function getValue(box: Box) {
  return box.value;
}
