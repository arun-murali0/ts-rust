interface Mapper {
  map<U extends string = string>(value?: U): U;
}
declare const mapper: Mapper;
const defaultValue: string = mapper.map();
const stringValue: string = mapper.map("hello");
