type First = { value: { count: number } };
type Second = { value: { count: number } };

function read(x: First | Second | null): number {
    if (x !== null) {
        return x.value.count;
    }
    return 0;
}
