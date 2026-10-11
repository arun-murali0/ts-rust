interface A0 { a0: number }
interface A1 { a1: number }
interface A2 { a2: number }
interface A3 { a3: number }
interface A4 { a4: number }
interface A5 { a5: number }
interface A6 { a6: number }
interface A7 { a7: number }
interface A8 { a8: number }
interface A9 { a9: number }
interface A10 { a10: number }
interface A11 { a11: number }
interface A12 { a12: number }
interface A13 { a13: number }
interface A14 { a14: number }
interface A15 { a15: number }
interface A16 { a16: number }
interface A17 { a17: number }
type UA = A0 | A1 | A2 | A3 | A4 | A5 | A6 | A7 | A8 | A9 | A10 | A11 | A12 | A13 | A14 | A15 | A16 | A17;
interface B0 { b0: number }
interface B1 { b1: number }
interface B2 { b2: number }
interface B3 { b3: number }
interface B4 { b4: number }
interface B5 { b5: number }
interface B6 { b6: number }
interface B7 { b7: number }
interface B8 { b8: number }
interface B9 { b9: number }
interface B10 { b10: number }
interface B11 { b11: number }
interface B12 { b12: number }
interface B13 { b13: number }
interface B14 { b14: number }
interface B15 { b15: number }
interface B16 { b16: number }
interface B17 { b17: number }
type UB = B0 | B1 | B2 | B3 | B4 | B5 | B6 | B7 | B8 | B9 | B10 | B11 | B12 | B13 | B14 | B15 | B16 | B17;
interface C0 { c0: number }
interface C1 { c1: number }
interface C2 { c2: number }
interface C3 { c3: number }
interface C4 { c4: number }
interface C5 { c5: number }
interface C6 { c6: number }
interface C7 { c7: number }
interface C8 { c8: number }
interface C9 { c9: number }
interface C10 { c10: number }
interface C11 { c11: number }
interface C12 { c12: number }
interface C13 { c13: number }
interface C14 { c14: number }
interface C15 { c15: number }
interface C16 { c16: number }
interface C17 { c17: number }
type UC = C0 | C1 | C2 | C3 | C4 | C5 | C6 | C7 | C8 | C9 | C10 | C11 | C12 | C13 | C14 | C15 | C16 | C17;
interface D0 { d0: number }
interface D1 { d1: number }
interface D2 { d2: number }
interface D3 { d3: number }
interface D4 { d4: number }
interface D5 { d5: number }
interface D6 { d6: number }
interface D7 { d7: number }
interface D8 { d8: number }
interface D9 { d9: number }
interface D10 { d10: number }
interface D11 { d11: number }
interface D12 { d12: number }
interface D13 { d13: number }
interface D14 { d14: number }
interface D15 { d15: number }
interface D16 { d16: number }
interface D17 { d17: number }
type UD = D0 | D1 | D2 | D3 | D4 | D5 | D6 | D7 | D8 | D9 | D10 | D11 | D12 | D13 | D14 | D15 | D16 | D17;

type X = UA & UB & UC & UD;

const x: X = null as any;
