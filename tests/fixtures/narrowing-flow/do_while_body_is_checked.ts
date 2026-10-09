// A do-while body runs before its test, so the guard in the test cannot narrow it.
function f(x: string | null): number {
  let total = 0;
  do {
    total = total + x.length;
  } while (total < 3);
  return total;
}
