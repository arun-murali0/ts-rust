interface Address {
    city: string;
}

interface User {
    address: Address | null;
}

function city(user: User): string {
    return user.address.city;
}
