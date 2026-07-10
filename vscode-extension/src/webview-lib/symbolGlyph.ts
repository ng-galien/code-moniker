// Shared symbol vocabulary across webviews: one glyph per kind, so a method
// looks the same on a graph card, in the detail header and in a member row.
const KIND_GLYPHS: Record<string, string> = {
	fn: "ƒ",
	function: "ƒ",
	method: "m",
	constructor: "c",
	macro: "!",
	test: "⚗",
	class: "◇",
	struct: "◇",
	enum: "≡",
	interface: "◇",
	trait: "◇",
	module: "▸",
	union: "◇",
	object: "◇",
	type: "τ",
};

export function symbolGlyph(kind: string): string {
	return KIND_GLYPHS[kind] ?? kind.charAt(0);
}

export function glyphClass(kind: string): string {
	if (kind === "method" || kind === "constructor") {
		return "glyph method";
	}
	if (["class", "struct", "enum", "interface", "trait", "union", "object", "type"].includes(kind)) {
		return "glyph type";
	}
	return "glyph";
}
