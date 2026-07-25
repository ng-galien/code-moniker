// Identity paths are "/"-joined "kind:name" segments; the format is owned by
// the daemon. Pure helpers, importable from both sides of the webview bridge.

export function segmentName(identity: string): string {
	const segment = identity.split("/").pop() ?? identity;
	return segment.split(":")[1] ?? segment;
}

export function parentPrefix(identity: string): string {
	return identity.includes("/") ? identity.slice(0, identity.lastIndexOf("/")) : "";
}

// Cumulative ancestor identities, shallowest first, ending with the prefix
// itself: "a/b/c" → ["a", "a/b", "a/b/c"].
export function ancestors(prefix: string): string[] {
	const segments = prefix ? prefix.split("/") : [];
	return segments.map((_, index) => segments.slice(0, index + 1).join("/"));
}
