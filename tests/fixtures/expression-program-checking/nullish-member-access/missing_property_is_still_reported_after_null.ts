interface Box {
    a: number;
}

function read(box: Box | null): number {
    return box.b;
}
