interface Mapper<T> {
    map<U>(f: (x: T) => U): U;
}

// Box<number> binds T only. U belongs to the method and is inferred per call.
function label(mapper: Mapper<number>): string {
    return mapper.map((x: number) => "label");
}
