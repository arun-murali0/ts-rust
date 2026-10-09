function outer(): number {
  // Called above its declaration: function declarations hoist.
  const r: number = helper(2);

  function helper(x: number): number {
    return x * 2;
  }

  return r;
}
