// Two independent generic functions that both name their type parameter "T". If
// identity were name-based instead of TypeParameterId-based, resolving one could
// leak into the other.
function firstOf<T>(items: T[]): T {
    return items[0];
}

function identity<T>(value: T): T {
    return value;
}

const a: number = firstOf([1, 2, 3]);
const b: string = identity("hello");
