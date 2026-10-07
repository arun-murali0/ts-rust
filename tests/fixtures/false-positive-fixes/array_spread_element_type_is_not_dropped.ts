const words: string[] = ["a"];
const mixed = [...words, 1];

// Before the fix the spread was ignored, `mixed` was number[], and this passed.
const onlyNumbers: number[] = mixed;
