use crate::{CheckRun, execute};

pub fn build(_run: &CheckRun) {
	execute();
}

mod nested {
	pub fn execute_from_parent() {
		super::super::execute();
	}
}
