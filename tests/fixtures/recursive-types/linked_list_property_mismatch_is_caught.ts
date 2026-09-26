interface ListNode {
    value: number;
    next: ListNode | null;
}

const tail: ListNode = { value: 2, next: null };
const head: ListNode = { value: 1, next: "not a node" };
