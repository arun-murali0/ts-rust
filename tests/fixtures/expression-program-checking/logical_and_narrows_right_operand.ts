interface Box {
    value: number;
}

function readValue(box: Box | null): number | null {
    return box && box.value;
}
