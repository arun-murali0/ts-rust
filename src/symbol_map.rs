use oxc_semantic::SymbolId;

use crate::arena::TypeId;

#[inline]
fn symbol_index(symbol_id: SymbolId) -> usize {
    symbol_id.index()
}

pub struct SymbolTypeMap {
    types: Vec<Option<TypeId>>,
}

impl SymbolTypeMap {
    pub fn new() -> Self {
        Self { types: Vec::new() }
    }

    pub fn declare(&mut self, symbol_id: SymbolId, type_id: TypeId) {
        let index = symbol_index(symbol_id);
        if index >= self.types.len() {
            self.types.resize(index + 1, None);
        }
        self.types[index] = Some(type_id);
    }

    pub fn get(&self, symbol_id: SymbolId) -> Option<TypeId> {
        self.types.get(symbol_index(symbol_id)).copied().flatten()
    }

    #[allow(dead_code)]
    pub fn is_declared(&self, symbol_id: SymbolId) -> bool {
        self.types
            .get(symbol_index(symbol_id))
            .is_some_and(Option::is_some)
    }
}

impl Default for SymbolTypeMap {
    fn default() -> Self {
        Self::new()
    }
}
