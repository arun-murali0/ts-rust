const identity = <T>(value: T): T => value;
const numberValue: number = identity(42);
const stringValue: string = identity("hello");
