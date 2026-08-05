#[derive(GeneratedApi)]
pub struct Generated;

pub struct Homonym;

impl Homonym {
	pub fn generated() {}
}

pub fn call_generated_api() {
	Generated::generated();
}

pub struct Plain;

pub fn call_missing_api() {
	Plain::missing();
}
