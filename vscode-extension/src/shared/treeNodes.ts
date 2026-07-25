// The workspace tree wraps every feature node as `{ kind, node }`, so a
// command invoked from a row receives the wrapper while the palette passes
// nothing. Unwrapping belongs here rather than once per feature.
export function unwrapWorkspaceNode(node: unknown): unknown {
	if (node && typeof node === "object" && "node" in node) {
		return (node as { node?: unknown }).node;
	}
	return node;
}
