interface OptionalValue {
    value?: number;
}

interface RequiredValue {
    value: number;
}

const optionalValue: OptionalValue = {};
const requiredValue: RequiredValue = optionalValue;
