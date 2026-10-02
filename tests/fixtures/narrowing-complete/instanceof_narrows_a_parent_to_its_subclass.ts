class Animal {
    name: string = "";
}

class Dog extends Animal {
    bark(): string {
        return "woof";
    }
}

function speak(a: Animal): string {
    if (a instanceof Dog) {
        return a.bark();
    }
    return a.name;
}
