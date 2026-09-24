// A tuple type is not something this checker's type resolver understands yet, so
// this bound cannot be resolved and T is left unconstrained.
function first<T extends [number, string]>(item: T): T {
  return item;
}
