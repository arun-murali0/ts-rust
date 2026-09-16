class Widget {
  count: number;

  getValue(value): number {
    return 1;
  }
}

const w: Widget = new Widget();

const badCount: string = w.count;

w.getValue(1 - "x");
