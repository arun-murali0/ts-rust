mod generics;
pub mod queries;

pub(crate) use generics::{expected_param_type, infer_type_param_bindings, substitute_type_params};
pub use queries::SemanticQueries;
