interface Fetcher {
    load(url: string): Promise<number>;
}

function run(fetcher: Fetcher): void {
    fetcher.load(42);
}
