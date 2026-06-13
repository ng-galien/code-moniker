// Lightweight TOML AST reader for .code-moniker.toml / *.fragment.toml. The CLI
// remains the source of truth for validation; this extracts tree/navigation data.

import { AST, parseTOML } from "toml-eslint-parser";

import { langByTomlSection } from "../shared/languages";

export interface RuleEntry {
	/** Scope without the trailing `.where`, e.g. `rust.fn`, `ts.shape.callable`, `refs`. */
	scope: string;
	id: string;
	severity: string;
	/** 0-indexed line of the `[[...where]]` header. */
	line: number;
	/** Raw TOML of this rule block. */
	blockText: string;
	/** Sample language id inferred from the scope section, if any. */
	sampleLang?: string;
}

export interface ParsedRuleFile {
	rules: RuleEntry[];
	aliases: string[];
	profiles: string[];
	fragment?: string;
}

export function parseRuleFile(text: string): ParsedRuleFile {
	const program = parseTOML(text);
	const top = program.body[0];
	const rules: RuleEntry[] = [];
	const aliases: string[] = [];
	const profiles: string[] = [];
	let fragment: string | undefined;

	for (const node of top.body) {
		if (node.type === "TOMLKeyValue" && keyPath(node.key).join(".") === "fragment") {
			fragment = stringValue(node.value) ?? fragment;
			continue;
		}
		if (node.type !== "TOMLTable") {
			continue;
		}
		const path = keyPath(node.key);
		if (path.length === 1 && path[0] === "aliases") {
			aliases.push(...node.body.map((item) => keyPath(item.key).join(".")));
			continue;
		}
		if (path.length === 2 && path[0] === "profiles") {
			profiles.push(path[1]);
			continue;
		}
		if (!isRuleTable(node, path)) {
			continue;
		}
		const scope = path.slice(0, -1).join(".");
		const id = tableStringField(node, "id") ?? `${scope}.where[${rules.length}]`;
		const severity = tableStringField(node, "severity") ?? "error";
		const section = scope.split(".")[0];
		rules.push({
			scope,
			id,
			severity,
			line: node.loc.start.line - 1,
			blockText: text.slice(node.range[0], node.range[1]).trimEnd() + "\n",
			sampleLang: langByTomlSection(section)?.id,
		});
	}

	return { rules, aliases, profiles, fragment };
}

function isRuleTable(node: AST.TOMLTable, path: string[]): boolean {
	return node.kind === "array" && path.length >= 2 && path[path.length - 1] === "where";
}

function tableStringField(table: AST.TOMLTable, name: string): string | undefined {
	const item = table.body.find((candidate) => keyPath(candidate.key).join(".") === name);
	return item ? stringValue(item.value) : undefined;
}

function keyPath(key: AST.TOMLKey): string[] {
	return key.keys.map((part) => part.type === "TOMLBare" ? part.name : part.value);
}

function stringValue(node: AST.TOMLContentNode): string | undefined {
	return node.type === "TOMLValue" && node.kind === "string" ? node.value : undefined;
}
