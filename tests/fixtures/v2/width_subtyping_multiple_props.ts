interface A { x: number; y: string }
interface B { x: number; y: string; z: boolean }

const b: B = { x: 1, y: "hello", z: true }
const a: A = b
