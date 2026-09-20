interface Inner {
  value: number;
}
interface Outer {
  inner: Inner;
}

const inner = { value: 42, extra: "ignored" };
const obj: Outer = { inner: inner };
