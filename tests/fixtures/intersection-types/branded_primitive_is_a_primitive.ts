type UserId = string & { __brand: "UserId" };

declare const id: UserId;
const text: string = id;
const idLength: number = id.length;
