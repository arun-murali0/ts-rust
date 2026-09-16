interface Box {
    value: number;
}

function readValue(box: Box | null): number | undefined {
    return box?.value;
}
