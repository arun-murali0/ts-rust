function greet(name: string): string {
  return "hello, " + name;
}

const label = greet("world");

interface Sized {
  width: number;
  height: number;
}

function area(shape: Sized): number {
  return shape.width * shape.height;
}

const box: Sized = { width: 3, height: 4 };
const total = area(box);
