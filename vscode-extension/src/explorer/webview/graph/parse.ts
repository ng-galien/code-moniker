// Callable names arrive as `base(name:Type,name:Type)`; the canonical card
// shows the base name dominant and one badge per argument, so the param list
// is parsed out of the identity instead of being printed inline.
export interface ParsedArg {
	name: string;
	type: string;
}

export interface ParsedCallable {
	base: string;
	args: ParsedArg[];
}

export function parseCallableName(name: string): ParsedCallable {
	const open = name.indexOf("(");
	if (open < 0) {
		return { base: name, args: [] };
	}
	const close = name.lastIndexOf(")");
	const inner = name.slice(open + 1, close < open ? name.length : close);
	const args = splitTopLevel(inner)
		.map(parseArg)
		.filter((arg): arg is ParsedArg => arg != null);
	return { base: name.slice(0, open), args };
}

function parseArg(part: string): ParsedArg | null {
	const trimmed = part.trim();
	if (trimmed.length === 0) {
		return null;
	}
	const colon = topLevelColon(trimmed);
	if (colon < 0) {
		return { name: trimmed, type: "" };
	}
	return {
		name: trimmed.slice(0, colon).trim(),
		type: cleanType(trimmed.slice(colon + 1)),
	};
}

// Splits on commas that sit outside any <>, (), [] nesting.
function splitTopLevel(text: string): string[] {
	const parts: string[] = [];
	let depth = 0;
	let start = 0;
	for (let i = 0; i < text.length; i++) {
		const ch = text[i];
		if (ch === "<" || ch === "(" || ch === "[") {
			depth++;
		} else if (ch === ">" || ch === ")" || ch === "]") {
			depth = Math.max(0, depth - 1);
		} else if (ch === "," && depth === 0) {
			parts.push(text.slice(start, i));
			start = i + 1;
		}
	}
	parts.push(text.slice(start));
	return parts;
}

function topLevelColon(text: string): number {
	let depth = 0;
	for (let i = 0; i < text.length; i++) {
		const ch = text[i];
		if (ch === "<" || ch === "(" || ch === "[") {
			depth++;
		} else if (ch === ">" || ch === ")" || ch === "]") {
			depth = Math.max(0, depth - 1);
		} else if (ch === ":" && depth === 0) {
			return i;
		}
	}
	return -1;
}

// Lifetimes and redundant whitespace are elided on badges; the exact
// signature lives in the code inset, not on the card.
export function cleanType(type: string): string {
	return type
		.replace(/'[A-Za-z_]+,?\s*/g, "")
		.replace(/<\s*>/g, "")
		.replace(/\s+/g, " ")
		.trim();
}
