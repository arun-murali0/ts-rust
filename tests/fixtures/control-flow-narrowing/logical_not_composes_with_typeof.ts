function measure(x: string | number): number {
  if (!(typeof x === "number")) {
    return x.length;
  }
  return x + 1;
}
