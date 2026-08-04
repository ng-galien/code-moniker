use crate::{CheckRun, execute};

pub fn build(_run: &CheckRun) {
	execute();
}

mod nested {
	use super::super::*;

	pub fn execute_from_parent() {
		super::super::execute();
	}

	pub fn execute_from_wildcard() {
		execute();
	}
}
