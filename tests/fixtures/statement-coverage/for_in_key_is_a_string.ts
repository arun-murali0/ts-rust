function firstKey(o: { a: number }): number {
  for (const k in o) {
    return k;
  }
  return 0;
}
