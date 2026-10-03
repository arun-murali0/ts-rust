class User {
    constructor(
        public name: string,
        readonly id: number,
        private secret: string,
    ) {}
}

const u = new User("a", 1, "s");
const n: string = u.name;
const i: number = u.id;
