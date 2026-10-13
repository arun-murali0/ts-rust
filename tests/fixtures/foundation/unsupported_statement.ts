// An ambient module declaration is a statement kind this checker does not handle yet. It
// used to be a do-while, then a namespace, both checked now; any statement that still
// reaches push_unsupported does the job.
declare module "legacy-lib" {
    export const sides: number;
}
