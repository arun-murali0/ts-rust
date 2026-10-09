type First = "a" | "b";
type Second = "b" | "c";

const only: First & Second = "b";
const widened: First = only;
