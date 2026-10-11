// Names the standard library provides, type parameters, and a type declared inside a
// function are all known. None of them is a missing name.
interface Box<T> {
    value: T;
}

function inspect(when: Date, lookup: Map<string, number>, node: HTMLElement | null): void {
    type Local = { n: number };
    const local: Local = { n: 1 };
    const boxed: Box<Local> = { value: local };
}

const later: Promise<Array<Date>> = Promise.resolve([]);
