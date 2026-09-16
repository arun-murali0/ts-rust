interface Box {
    value: number;
}

function readValue(box: Box | null): number {
    return box && box.value;
}
