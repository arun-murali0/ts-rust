type HigherOrder = <T>(callback: <U>(value: U) => T) => T;
declare const higherOrder: HigherOrder;
