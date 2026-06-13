import { existsSync, readFileSync } from "node:fs";
import * as path from "node:path";
import * as vscode from "vscode";

import { RuleFileNode } from "./nodes";
import { ParsedRuleFile, parseRuleFile } from "./parse";
import { rootOf } from "../shared/workspace";

export const RULE_GLOB = "**/{.code-moniker.toml,*.fragment.toml}";
export const RULE_GLOB_EXCLUDE = "**/node_modules/**";

export async function findRuleFiles(): Promise<RuleFileNode[]> {
	const uris = await vscode.workspace.findFiles(RULE_GLOB, RULE_GLOB_EXCLUDE);
	uris.sort((a, b) => a.fsPath.localeCompare(b.fsPath));
	return uris.map((uri) => ({ kind: "file", uri, parsed: readParsedRuleFile(uri) }));
}

export function readParsedRuleFile(uri: vscode.Uri): ParsedRuleFile {
	try {
		return parseRuleFile(readFileSync(uri.fsPath, "utf8"));
	} catch {
		return { rules: [], aliases: [], profiles: [] };
	}
}

export type RulesEntrypoint =
	| { ok: true; uri: vscode.Uri }
	| { ok: false; error: string };

export function rulesEntrypoint(uri: vscode.Uri, label: string): RulesEntrypoint {
	if (!isFragmentFile(uri)) {
		return { ok: true, uri };
	}
	const root = rootOf(uri);
	let current = path.dirname(uri.fsPath);
	while (isSameOrInside(current, root)) {
		const candidate = path.join(current, ".code-moniker.toml");
		if (existsSync(candidate)) {
			return { ok: true, uri: vscode.Uri.file(candidate) };
		}
		const parent = path.dirname(current);
		if (parent === current) {
			break;
		}
		current = parent;
	}
	return {
		ok: false,
		error:
			`${label} is a rule fragment. ` +
			"Fragments are loaded through a parent .code-moniker.toml; none was found in this workspace.",
	};
}

export function isFragmentFile(uri: vscode.Uri): boolean {
	return path.basename(uri.fsPath).endsWith(".fragment.toml");
}

function isSameOrInside(candidate: string, root: string): boolean {
	const relative = path.relative(root, candidate);
	return relative === "" || (!relative.startsWith("..") && !path.isAbsolute(relative));
}
