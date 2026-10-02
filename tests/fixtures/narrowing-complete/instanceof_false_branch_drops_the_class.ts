// Expect exactly one error: after the guard pet is a Cat, so bark does not exist.
class Dog {
    name: string = "";
    bark(): string {
        return "woof";
    }
}

class Cat {
    name: string = "";
    purr(): string {
        return "purr";
    }
}

function speak(pet: Dog | Cat): string {
    if (pet instanceof Dog) {
        return pet.bark();
    }
    return pet.bark();
}
