function outer(): number {
  return 1;

  // tsc does not flag an unreachable function *declaration* on its own,
  // since hoisting means the declaration itself always "runs" even when
  // control never reaches this point -- only what executes after it can be
  // dead. This checker does not special-case that yet (see
  // src/bridge/unreachable_code.rs), so the declaration itself is flagged
  // here, unlike tsc. Documents the current, known-narrower behavior
  // rather than leaving it undiscovered.
  function neverCalled(): number {
    return 0;
  }
}
