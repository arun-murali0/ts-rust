class Config {
    initialized: number = 1;
    assigned: string;
    asserted!: number;
    optional?: number;
    allowsUndefined: number | undefined;
    anything: any;
    static shared: number;

    constructor() {
        this.assigned = "value";
    }
}
