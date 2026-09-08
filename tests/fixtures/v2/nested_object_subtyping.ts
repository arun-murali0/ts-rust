interface Inner { value: number }
interface Outer { inner: Inner }

const obj: Outer = { inner: { value: 42, extra: "ignored" } }
