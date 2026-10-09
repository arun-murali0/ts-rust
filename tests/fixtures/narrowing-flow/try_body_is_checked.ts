function f(x: string | null): number {
  try {
    return x.length;
  } catch (e) {
    return 0;
  }
}
