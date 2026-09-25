function f(kind: string, x: number | null): number {
  switch (kind) {
    // No `break`: this checker does not model switch fallthrough, and
    // break is one of the few statement kinds it does not check yet, so
    // leaving it out keeps this fixture free of an unrelated warning.
    case "a":
      if (x === null) {
        return 0;
      }
    case "b":
      // Case "a"'s narrowing of x must not have carried over here.
      const y: number = x;
      return y;
  }
  return 1;
}
