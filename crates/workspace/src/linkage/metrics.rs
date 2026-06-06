#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LinkageMemoryMetrics {
	pub reference_sets: usize,
	pub reference_set_values: u64,
	pub reference_set_serialized_bytes: usize,
	pub symbol_sets: usize,
	pub symbol_set_values: usize,
	pub symbol_set_serialized_bytes: usize,
	pub symbol_catalog_entries: usize,
	pub decisions: usize,
}

impl LinkageMemoryMetrics {
	pub(crate) fn add_reference_set(&mut self, len: u64, serialized_bytes: usize) {
		self.reference_sets += 1;
		self.reference_set_values += len;
		self.reference_set_serialized_bytes += serialized_bytes;
	}

	pub(crate) fn add_symbol_set(&mut self, len: usize, serialized_bytes: usize) {
		self.symbol_sets += 1;
		self.symbol_set_values += len;
		self.symbol_set_serialized_bytes += serialized_bytes;
	}
}
