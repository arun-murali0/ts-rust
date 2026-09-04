// Array<T> is covariant here, matching real TypeScript behavior. This is
// technically unsound under mutation (a caller could push a non-number
// into anyNums through the nums reference), but neither TypeScript nor
// ts-rust model mutation-site variance, so this assignment is allowed.
const nums: number[] = [1, 2, 3];
const anyNums: unknown[] = nums;
