// A callable's generic list must survive even when T appears only in the return type.
declare function make<T = string>(): T;
const value: string = make();
