pub struct Settings;

impl Settings {
	pub fn is_enabled(&self) -> bool {
		true
	}
}

pub struct Command {
	settings: Settings,
}

impl Command {
	pub fn enabled(&self) -> bool {
		self.settings.is_enabled()
	}
}

pub struct Start;
pub struct Intermediate;

impl Start {
	pub fn advance(self) -> Intermediate {
		Intermediate
	}
}

impl Intermediate {
	pub fn finish(self) {}
}

pub fn call_chain(start: Start) {
	start.advance().finish();
}

pub struct Wrapper<T> {
	value: T,
}

impl<T> Wrapper<T> {
	pub fn into_inner(self) -> T {
		self.value
	}
}

pub struct Builder;

impl Builder {
	pub fn new() -> Self {
		Self
	}

	pub fn option(self) -> Self {
		self
	}

	pub fn finish(self) {}
}

pub fn call_self_returning_builder() {
	Builder::new().option().finish();
}

mod contract {
	pub trait Parser {
		fn parse() -> Self;
	}
}

use contract::Parser;

#[derive(Parser)]
pub struct DerivedParser;

pub fn call_derived_trait_method() {
	DerivedParser::parse();
}
