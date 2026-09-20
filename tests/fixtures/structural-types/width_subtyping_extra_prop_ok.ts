interface Point {
  x: number;
}

// Not fresh: the extra property `y` is fine once the object has gone through a
// variable. Writing the literal directly at the assignment is the excess
// property error (see excess_property_literal.ts).
const wide = { x: 1, y: 2 };
const p: Point = wide;
