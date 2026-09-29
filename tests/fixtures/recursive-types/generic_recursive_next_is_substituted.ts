interface Box<T> {
  value: T;
  next: Box<T> | null;
}

function nextValue(box: Box<number>): number {
  const rest = box.next;
  if (rest !== null) {
    return rest.value;
  }
  return 0;
}

function wrongNextValue(box: Box<number>): string {
  const rest = box.next;
  if (rest !== null) {
    return rest.value;
  }
  return "";
}
