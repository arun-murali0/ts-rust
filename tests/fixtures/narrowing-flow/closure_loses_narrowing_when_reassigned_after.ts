// x is assigned again after the arrow is created, so inside it x is string | null.
function f(x: string | null): () => number {
  if (x === null) {
    return () => 0;
  }
  const g = () => x.length;
  x = null;
  return g;
}
