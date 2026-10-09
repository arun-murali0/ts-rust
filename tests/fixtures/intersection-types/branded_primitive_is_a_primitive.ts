type UserId = string & { __brand: "UserId" };

declare const id: UserId;
const text: string = id;
const length: number = id.length;
