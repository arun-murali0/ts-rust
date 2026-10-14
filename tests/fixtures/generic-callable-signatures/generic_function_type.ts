type Identity = <T>(value: T) => T;
declare const identity: Identity;
const numberValue: number = identity(42);
const stringValue: string = identity("hello");
