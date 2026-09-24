interface Reader {
    read(): string;
    peek?(): string;
}

const reader: Reader = {
    read() {
        return "data";
    },
};
