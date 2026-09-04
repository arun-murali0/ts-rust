// Despite the filename (kept so the existing test's include_str! path
// doesn't need to change), this is no longer a known gap: `this` now
// resolves to the class's instance type, so `this.count` correctly
// resolves to `number`, matching increment()'s declared return type.
class Counter {
    count: number;

    increment(): number {
        return this.count;
    }
}
