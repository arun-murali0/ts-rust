export type GenericIdentity = <T = string>(value?: T) => T;
export interface GenericMapper {
  map<U extends string = string>(value?: U): U;
}
