interface Box<T extends string> {
  value: T;
}

function keepBound<U extends string>(box: Box<U>): U {
  return box.value;
}

function dropBound<V>(box: Box<V>): V {
  return box.value;
}
