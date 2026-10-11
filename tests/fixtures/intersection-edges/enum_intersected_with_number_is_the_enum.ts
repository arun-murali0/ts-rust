enum E {
    A,
    B,
}

type Same = E & number;

function keep(e: E): Same {
    return e;
}
