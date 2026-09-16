interface Box {
    value: number;
}

function readValue(box: Box): number {
    return box["value"];
}
