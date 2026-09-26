interface Box {
    value: number;
}

interface Box {
    label: string;
}

// If the second declaration had overwritten the first instead of merging with
// it, `value` would not exist on Box at all.
const b: Box = { value: 1, label: "x" };
const v: number = b.value;
const l: string = b.label;
