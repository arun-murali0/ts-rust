interface Animal {
    name: string;
}

interface Dog {
    name: string;
    breed: string;
}

interface Feeder {
    feed(animal: Animal): string;
}

interface DogFeeder {
    feed(dog: Dog): string;
}

// tsc compares method parameters bivariantly, so a method that takes the more
// specific type is still assignable where the more general one is expected.
function widen(feeder: DogFeeder): Feeder {
    return feeder;
}
