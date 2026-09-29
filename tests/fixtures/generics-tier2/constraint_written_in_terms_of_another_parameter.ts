function pushInto<T, U extends T[]>(items: U, extra: T): U {
  return items;
}

const numbers: number[] = [1, 2];

const fine = pushInto(numbers, 3);
const bad = pushInto(numbers, "x");
