function outer(): number {
  function helper(x: number): number {
    return "twice";
  }
  return helper(1);
}
