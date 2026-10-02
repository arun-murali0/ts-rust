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
    return pet.purr();
}
