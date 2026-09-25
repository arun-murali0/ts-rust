interface Dog {
    name: string;
}

interface Box<T> {
    value: T;
}

function label(b: Box<Dog>): string {
    return b;
}
