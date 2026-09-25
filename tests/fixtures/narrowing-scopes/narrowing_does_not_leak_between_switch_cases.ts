function f(kind: string, x: number | null): number {
  switch (kind) {
    case "a":
      if (x === null) {
        return 0;
      }
  }
  // Narrowing from inside a case must not survive past the switch either.
  const y: number = x;
  return y;
}
