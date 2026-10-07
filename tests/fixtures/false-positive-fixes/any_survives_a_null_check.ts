// `any` is neither null nor not null as far as this test can tell, so it stays any
// in both branches. Before the fix it became never inside the if.
// The function is not called `length`: a top-level `length` collides with the global one
// from the DOM lib, and tsc reports that (TS2300) instead of testing the narrowing.
function lengthOf(x: any): number {
  if (x === null) {
    return x.length;
  }
  return 0;
}
