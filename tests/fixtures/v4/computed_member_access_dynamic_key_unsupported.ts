interface Box {
    value: number;
}

function readDynamic(box: Box, key: string): number {
    return box[key];
}
