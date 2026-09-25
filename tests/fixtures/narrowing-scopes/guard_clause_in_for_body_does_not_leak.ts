function f(x: number | null): number {
  // No update clause: `i++` is an update expression, which this checker does
  // not check yet, and would add a warning unrelated to what this fixture
  // is testing.
  for (let i = 0; i < 10;) {
    if (x === null) {
      return 0;
    }
  }
  // Same as the while case: narrowing from inside the loop body must not
  // reach code after the loop.
  const y: number = x;
  return y;
}
