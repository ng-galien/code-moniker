export function makeStringer() {
	type Local = string;
	function inner(x: Local): Local {
		return x;
	}
	return inner;
}

export function makeShaper() {
	interface Shape {
		v: number;
	}
	function client(s: Shape): Shape {
		return s;
	}
	return client;
}

export function makeMode() {
	enum Mode {
		A,
		B,
	}
	function pick(m: Mode): Mode {
		return m;
	}
	return pick;
}

export function makeBuilder() {
	class Local {
		ok = true;
	}
	function build(): Local {
		return new Local();
	}
	return build;
}
