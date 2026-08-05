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

pub fn clone_primitive(value: &str) -> String {
	value.to_owned()
}

pub fn vec_len(values: &Vec<u8>) -> usize {
	values.len()
}

pub fn load_pointer(
	pointer: &AtomicPtr<u8>,
) -> *mut u8 {
	pointer.load(Ordering::Relaxed)
}
use std::sync::atomic::{AtomicPtr, Ordering};

pub fn cast_pointer(pointer: *const u8) -> *const () {
	pointer.cast()
}

pub fn drop_value<T>(value: T) {
	drop(value);
}
