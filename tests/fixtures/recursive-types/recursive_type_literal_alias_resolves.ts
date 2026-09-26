type ListNode = {
    value: number;
    next: ListNode | null;
};

function second(node: ListNode): number {
    return node.next;
}
