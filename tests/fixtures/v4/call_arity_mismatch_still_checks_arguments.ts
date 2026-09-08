interface Box {
    value: number;
}

function add(a: number, b: number): number {
    return a + b;
}

function useAdd(box: Box): number {
    return add(1, box.missing, 3);
}
