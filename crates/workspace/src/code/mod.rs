mod index;
pub(crate) mod symbols;

pub use index::{CodeIndexPort, CodeIndexRefresh, LocalCodeIndex, LocalCodeIndexOptions};
pub use symbols::{
	CodeIndexSymbolProvider, NormalizedSource, NormalizedSymbol, SymbolProvider, compact_moniker,
	def_kind, is_navigable_def, last_name, ref_kind,
};
