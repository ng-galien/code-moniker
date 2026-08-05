pub trait ValueEnum {
	fn value_variants() -> &'static [Self];
}

pub struct Homonym;

impl Homonym {
	pub fn value_variants() {}
}

pub fn call_generic_contract<T: ValueEnum>() {
	let _ = T::value_variants();
}

mod contract {
	pub trait Parser {
		fn parse() -> Self;
	}
}

use contract::Parser;

#[derive(Parser)]
pub struct DerivedParser;

pub fn call_derived_contract() {
	let _ = DerivedParser::parse();
}

pub trait Remaining {
	fn remaining(&self) -> usize;
}

impl Remaining for &[u8] {
	fn remaining(&self) -> usize {
		self.len()
	}
}

impl Remaining for Vec<u8> {
	fn remaining(&self) -> usize {
		self.len()
	}
}

pub fn stringify<T: ToString>(value: T) -> String {
	value.to_string()
}
