// Expect exactly one error: `age` is not a property of User.
class User {
    constructor(public name: string) {}
}

const u = new User("a");
const n = u.age;
