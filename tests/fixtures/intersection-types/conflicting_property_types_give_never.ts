declare const v: { x: number } & { x: string };

const asNumber: number = v.x;
const asString: string = v.x;
