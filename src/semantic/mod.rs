mod generics;
pub mod queries;

pub(crate) use generics::{
    collect_generic_param_constraints, contains_type_param, expected_param_type,
    infer_type_param_bindings, ordered_generic_param_ids, substitute_bound_type_params,
    substitute_type_params,
};
pub use queries::SemanticQueries;
