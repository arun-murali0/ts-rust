interface Service {
    run(): number;
}

function start(service: Service | null): number {
    return service.run();
}
