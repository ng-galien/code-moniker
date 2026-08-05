mod bytes;
mod same_name;

pub use crate::bytes::Bytes;
pub use crate::same_name::same_name;

pub mod prelude {
	pub use crate::bytes::*;
}

pub mod aot {
	mod shells {
		pub struct Bash;
	}

	pub use shells::*;
}

pub mod shells {
	pub use crate::aot::Bash;
}
