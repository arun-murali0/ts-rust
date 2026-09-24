interface Box<T> {
    value: T;
}

function makeNumberBox(): Box<number> {
    return { value: 5 };
}

const box: Box<number> = makeNumberBox();
const n: number = box.value;
