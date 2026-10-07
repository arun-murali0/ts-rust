// `any` is neither null nor not null as far as this test can tell, so it stays any
// in both branches. Before the fix it became never inside the if.
function length(x: any): number {
  if (x === null) {
    return x.length;
  }
  return 0;
}
