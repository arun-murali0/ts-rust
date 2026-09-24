interface Animal {
    name: string;
}

interface Dog {
    name: string;
    breed: string;
}

class AnimalFeeder {
    feed(animal: Animal): string {
        return animal.name;
    }
}

class DogFeeder {
    feed(dog: Dog): string {
        return dog.breed;
    }
}

// Class methods are methods too, so the same bivariance applies.
const feeder: AnimalFeeder = new DogFeeder();
