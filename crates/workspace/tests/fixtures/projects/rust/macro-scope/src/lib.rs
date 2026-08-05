macro_rules! root_macro {
	() => {};
}

#[macro_use]
mod macros;
mod consumer;

pub fn call_root_macro() {
	root_macro!();
}

pub fn call_module_macro() {
	module_macro!();
}

pub mod nested {
	pub fn call_parent_macro() {
		root_macro!();
	}
}
