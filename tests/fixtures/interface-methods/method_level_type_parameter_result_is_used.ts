interface Mapper<T> {
    map<U>(f: (x: T) => U): U;
}

function label(mapper: Mapper<number>): string {
    return mapper.map((x: number) => x);
}
