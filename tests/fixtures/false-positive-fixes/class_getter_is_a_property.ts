class Order {
  price: number;

  constructor(price: number) {
    this.price = price;
  }

  get total(): number {
    return this.price * 2;
  }
}

const order = new Order(3);
const total: number = order.total;
