interface Box {
    value: number;
}

function readOrDefault(box: Box | null): number {
    return box ? box.value : 0;
}
