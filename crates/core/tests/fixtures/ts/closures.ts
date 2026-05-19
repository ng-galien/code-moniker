// cm: def make stringer
export function makeStringer() {
	// cm: def local type in function
	type Local = string;
	// cm: def inner function using local type
	function inner(x: Local): Local {
		return x;
	}
	return inner;
}

// cm: def make shaper
export function makeShaper() {
	// cm: def local interface in function
	interface Shape {
		v: number;
	}
	// cm: ref client uses local shape
	function client(s: Shape): Shape {
		return s;
	}
	return client;
}

// cm: def make mode
export function makeMode() {
	// cm: def local enum in function
	enum Mode {
		A,
		B,
	}
	// cm: ref pick uses local enum
	function pick(m: Mode): Mode {
		return m;
	}
	return pick;
}

// cm: def make builder
export function makeBuilder() {
	// cm: def local class in function
	class Local {
		ok = true;
	}
	// cm: def builder function
	function build(): Local {
		// cm: ref build instantiates local class
		return new Local();
	}
	return build;
}
