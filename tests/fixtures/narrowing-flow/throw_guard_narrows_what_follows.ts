function f(x: string | null): number {
  if (x === null) {
    throw new Error("missing");
  }
  return x.length;
}
