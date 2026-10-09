type ToText = (x: any) => string;
type ToCount = (x: number) => number;

declare const loose: ToText & ToCount;
declare const tight: ToCount & ToText;

const text: string = loose(1);
const count: number = tight(1);
