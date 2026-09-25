function f(x: number | null): number {
  while (x !== null) {
    // Array member access is not supported yet, so the loop's own test is
    // kept simple; what this fixture checks is the guard clause below.
    if (x === null) {
      return 0;
    }
  }
  // x is still `number | null` here: the guard clause only narrowed x inside
  // the loop body, which this checker checks once, not per iteration.
  const y: number = x;
  return y;
}
