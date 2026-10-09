function f(x: string | null): () => number {
  const y = x;
  if (y === null) {
    return () => 0;
  }
  return () => y.length;
}
