// One small function per construct, so a dump can be read a function at a time
// and one construct's blocks are never tangled with another's. Each function
// exists to answer one question about how oxc builds the graph, noted above it.
// Run it with: cargo run --example dump_cfg -- examples/cfg_sample.ts

// The guard clause. The consequent exits, so the block after the `if` should be
// reachable only through the false edge, which is what would let `x` stay
// narrowed to non-null afterward.
function ifOnly(x: number | null): number {
  if (x === null) {
    return 0;
  }
  return x;
}

// Both branches exit. Whether the block after the `if` is marked unreachable, and
// which of the two `Jump` edges is the consequent's, decides how joins are merged.
function ifElse(x: number | null): number {
  if (x === null) {
    return 0;
  } else {
    return x;
  }
}

// Neither branch exits, so both reach the join and their narrowing must be merged
// there. Also has an assignment, which the graph does not model as an event.
function guardWithElseFallthrough(x: number | null): number {
  let result = 1;
  if (x === null) {
    result = 0;
  } else {
    result = x;
  }
  return result;
}

// Whether `?:` is wired like an `if`, with the same Jump edges, or like `&&`.
function ternary(x: number | null): number {
  return x === null ? 0 : x;
}

// `&&` is short-circuiting control flow, but its edges are all `Normal`; the
// operator has to come from the AST.
function logicalAnd(x: { value: number } | null): number | null {
  return x && x.value;
}

// Same shape as `&&`, to confirm nothing in the graph tells the two apart.
function logicalOr(x: number | null): number {
  return x || 0;
}

// Same shape again for `??`.
function nullish(x: number | null): number {
  return x ?? 0;
}

// The loop back-edge. A variable assigned in the body must lose its narrowing at
// the loop header, since the next iteration sees the assigned value.
function whileLoop(x: number | null): number {
  while (x !== null) {
    x = null;
  }
  return 0;
}

// `continue` and `break` inside a loop, where a guard clause must not leak its
// narrowing past the loop.
function loopWithGuard(items: number[], x: number | null): number {
  for (let i = 0; i < items.length; i++) {
    if (x === null) {
      continue;
    }
    if (items[i] > 10) {
      break;
    }
  }
  return 0;
}

// Fallthrough between cases, `break`, and an exit in one case, all in one graph.
function switchStatement(kind: string): number {
  switch (kind) {
    case "a":
      return 1;
    case "b":
      break;
    default:
      return 3;
  }
  return 2;
}

// The error and finalize edges. Every block in a `try` may jump to the `catch`,
// so narrowing established inside it cannot be trusted there.
function tryCatch(x: number | null): number {
  try {
    if (x === null) {
      throw new Error("null");
    }
    return x;
  } catch (error) {
    return 0;
  } finally {
    x = null;
  }
}

// An `Iteration` instruction and a back-edge with no condition to narrow on.
function forOf(items: number[]): number {
  let total = 0;
  for (const item of items) {
    total = item;
  }
  return total;
}

// Whether `?.` splits the graph at all. If it does not, narrowing on it must come
// from the AST alone.
function optionalChain(x: { value: number } | null): number | undefined {
  return x?.value;
}
