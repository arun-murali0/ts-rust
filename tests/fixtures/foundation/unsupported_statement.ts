// A namespace is a statement kind this checker does not handle yet. It used to be a
// do-while, which is checked now; any statement that still reaches push_unsupported
// does the job.
namespace Shapes {
    export const sides = 4;
}
