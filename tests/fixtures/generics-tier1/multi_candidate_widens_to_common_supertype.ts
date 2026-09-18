interface Base {
    id: number;
}

interface Extended {
    id: number;
    detail: string;
}

function pick<T>(a: T, b: T): T {
    return a;
}

function makeExtended(): Extended {
    return { id: 1, detail: "x" };
}

function makeBase(): Base {
    return { id: 2 };
}

const result: Base = pick(makeExtended(), makeBase());
