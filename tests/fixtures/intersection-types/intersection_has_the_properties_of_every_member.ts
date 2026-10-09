interface Named {
  name: string;
}
interface Aged {
  age: number;
}

function describe(person: Named & Aged): string {
  const name: string = person.name;
  const age: number = person.age;
  return name + age;
}
