// Regression fixture: a class used to resolve field-by-field, so a class
// with one unresolvable member (here, a method with no return type
// annotation) still got registered as a partial instance type built from
// whatever else it could resolve, with no diagnostic anywhere.
//
// Now the whole class fails to resolve, same policy interfaces already
// use, and that failure is reported right here at the class's own
// declaration site (see bridge/statements.rs's ClassDeclaration arm).
class Point {
  x: number;

  getY(y) {
    return y;
  }
}
