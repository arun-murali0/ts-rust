// An unannotated `const xs = []` is an array that fills up later in tsc; here it is any[],
// so it can be handed to anything that wants an array.
const xs = [];
const ys: number[] = xs;
const zs: string[] = xs;
