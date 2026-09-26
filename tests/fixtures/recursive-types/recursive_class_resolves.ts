class TreeNode {
    value: number = 0;
    left: TreeNode | null = null;
    right: TreeNode | null = null;
}

function label(node: TreeNode): string {
    return node.left;
}
