import { readFileSync } from "node:fs";
import * as vscode from "vscode";

import { RuleFileNode } from "./nodes";
import { ParsedRuleFile, parseRuleFile } from "./parse";

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
