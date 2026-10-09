function f(x: string | null, go: () => boolean): number {
  if (x === null) {
    return 0;
  }
  while (go()) {
    x.length;
  }
  return x.length;
}
