export function arrayToEnum(items: string[]): Record<string, string> {
	const out: Record<string, string> = {};
	for (const item of items) {
		out[item] = item;
	}
	return out;
}
