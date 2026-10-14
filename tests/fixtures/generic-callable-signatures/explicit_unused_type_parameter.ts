// Explicit type arguments bind parameters that cannot be discovered from the type graph.
declare function make<T>(): T;
const value: number = make<number>();
