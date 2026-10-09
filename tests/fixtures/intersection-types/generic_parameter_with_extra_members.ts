function getId<T>(value: T & { id: number }): number {
  return value.id;
}

const total: number = getId({ id: 1, label: "x" });
