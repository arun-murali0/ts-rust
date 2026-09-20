interface Box {
    value: number;
}

function readAny(box: Box, key: any): number {
    return box[key];
}
