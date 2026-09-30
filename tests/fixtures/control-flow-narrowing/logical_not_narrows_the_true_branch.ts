function describe(x: string | null): number {
  if (!x) {
    return 0;
  }
  return x.length;
}
