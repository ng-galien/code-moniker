use crate_surface::{Bytes, prelude::Bytes as PreludeBytes, same_name};

pub fn call_named_reexport() {
	let _value = Bytes::from_static(b"named");
}

pub fn call_wildcard_reexport() {
	let _value = PreludeBytes::from_static(b"wildcard");
}

pub fn call_same_name_reexport() {
	same_name();
}

pub fn read_nested_public_module() {
	let _shell = crate_surface::shells::Bash;
}
