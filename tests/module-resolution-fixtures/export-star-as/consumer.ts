import { ns } from './barrel';
const value: number = ns.value;
const greeting: string = ns.greet('world');
const bad: boolean = ns.value;

const typedBad: ns.User = { name: 1 };
