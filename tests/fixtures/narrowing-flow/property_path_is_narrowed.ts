interface Address { city: string }
interface User { address: Address | null }

function f(user: User): string {
  if (user.address !== null) {
    return user.address.city;
  }
  return "";
}
