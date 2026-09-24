interface Reader {
    read(): string;
    peek?(): string;
}

const reader: Reader = {
    peek() {
        return "data";
    },
};
