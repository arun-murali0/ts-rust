class MathHelper {
    add(a: number, b: number): number {
        return a + b;
    }
}

const helper = new MathHelper();
const sum: number = helper.add(1, 2);
