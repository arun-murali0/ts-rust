interface Animal {
    name: string;
}

interface Dog {
    name: string;
    breed: string;
}

interface Feeder {
    feed: (animal: Animal) => string;
}

interface DogFeeder {
    feed: (dog: Dog) => string;
}

// A function-typed property is not a method: its parameters stay contravariant
// under strictFunctionTypes, so this is rejected.
function widen(feeder: DogFeeder): Feeder {
    return feeder;
}
