interface Logger {
    log(message: string): void;
}

function run(logger: Logger): void {
    logger.log(42);
}
