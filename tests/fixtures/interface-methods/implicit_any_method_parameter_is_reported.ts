interface Runner {
    run(input): string;
}

function go(runner: Runner): string {
    return runner.run(1);
}
