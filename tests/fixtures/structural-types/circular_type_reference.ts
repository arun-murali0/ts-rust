interface A { b: B }
interface B { a: A }

const x: A = { b: { a: {} as any } }
