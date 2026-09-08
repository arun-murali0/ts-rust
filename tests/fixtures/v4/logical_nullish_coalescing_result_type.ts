interface Box {
    value: number;
}

function readOrDefault(box: Box | null): Box {
    const result: Box = box ?? { value: 0 };
    return result;
}
