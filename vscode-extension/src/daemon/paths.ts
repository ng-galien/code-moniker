import * as path from "node:path";

// The daemon encodes locations as a `{ root, path }` pair where the root is always
// absolute but the path may be absolute (violations) or workspace-relative
// (symbols). These two helpers are the single place that reconciles that.

export function toFsPath(root: string, p: string): string {
	return path.isAbsolute(p) ? p : path.join(root, p);
}

export function toRelative(root: string, p: string): string {
	return path.isAbsolute(p) ? path.relative(root, p) : p;
}
