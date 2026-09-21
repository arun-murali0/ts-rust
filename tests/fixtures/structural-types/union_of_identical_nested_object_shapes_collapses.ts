function read(flag: boolean): number {
    const value = flag ? { inner: { count: 1 } } : { inner: { count: 2 } };
    return value.inner.count;
}
