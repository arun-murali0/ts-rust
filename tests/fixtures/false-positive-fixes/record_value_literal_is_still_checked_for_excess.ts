// The keys of a Record are free, but each value is still checked against V, so
// `y` is an excess property of the inner literal.
const shapes: Record<string, { x: number }> = { a: { x: 1, y: 2 } };
