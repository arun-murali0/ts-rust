interface Reversed {
    z: number;
    m: string;
    a: boolean;
}

class Shuffled {
    m: string = "";
    z: number = 0;
    a: boolean = false;
}

const literal = { z: 1, m: "x", a: true };

const fromInterface: Reversed = literal;
const fromClass: Reversed = new Shuffled();
const fromAnnotation: { a: boolean; m: string; z: number } = literal;
