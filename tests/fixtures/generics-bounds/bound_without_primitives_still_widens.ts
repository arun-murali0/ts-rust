function keep<T extends { n: number }>(x: T): T {
    return x;
}

const r: { n: number } = keep({ n: 1 });
