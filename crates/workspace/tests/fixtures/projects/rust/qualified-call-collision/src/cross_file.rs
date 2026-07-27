pub struct CrossFileClone;

impl CrossFileClone {
	pub fn clone(&self) -> Self {
		Self
	}
}

#[derive(Clone)]
pub struct DerivedClone;
