class LinkNode<T> {
    value: T;
    next: LinkNode<T> | null = null;
    constructor(value: T) {
        this.value = value;
    }
}

function chain(node: LinkNode<number>): string {
    return node.value;
}
