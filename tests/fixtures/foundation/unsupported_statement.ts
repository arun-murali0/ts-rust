// A global augmentation is a statement kind this checker does not handle yet. It used to
// be a do-while, then a namespace, then an ambient module, all checked now; any statement
// that still reaches push_unsupported does the job.
declare global {
    var legacyCounter: number;
}
