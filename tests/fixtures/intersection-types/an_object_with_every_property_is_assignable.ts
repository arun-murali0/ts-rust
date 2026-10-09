interface Named {
  name: string;
}
interface Aged {
  age: number;
}

const person: Named & Aged = { name: "Ada", age: 36 };
const named: Named = person;
const aged: Aged = person;
