use unknown_vendored::helper;

use crate::store;

pub fn run() -> u32 {
	helper();
	store::version()
}
