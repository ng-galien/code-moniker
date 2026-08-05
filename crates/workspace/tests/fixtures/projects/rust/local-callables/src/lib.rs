pub fn run() {
	fn nested() {}
	let callback = || {};

	nested();
	callback();
}

pub fn invoke(action: impl FnOnce()) {
	action();
}

fn generation() -> u64 {
	1
}

pub fn shadowing_initializer() -> u64 {
	let generation = generation();
	generation
}
