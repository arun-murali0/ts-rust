import type { GenericIdentity, GenericMapper } from "./cross_file_provider";
declare const identity: GenericIdentity;
declare const mapper: GenericMapper;
const identityValue: string = identity();
const mappedValue: string = mapper.map();
