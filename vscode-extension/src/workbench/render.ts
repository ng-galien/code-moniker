import { DaemonNode } from "../daemon/nodes";
import { RuleTreeNode } from "../rules/nodes";
import { RulesTreeNode } from "../rules-daemon/nodes";
import { DetailDocument, DetailRow } from "../symbols/detail/panel";
import { ViewTreeNode } from "../views/nodes";
import { WorkspaceNode } from "./workspaceTree";

export function renderWorkspaceNode(node: WorkspaceNode): DetailDocument | undefined {
	switch (node.kind) {
		case "section":
			return renderSection(node.label);
		case "daemon":
			return renderDaemon(node.node);
		case "views":
			return renderViewNode(node.node);
		case "check":
			return renderCheckNode(node.node);
		case "ruleFiles":
			return renderRuleFileNode(node.node);
		case "symbols":
			return node.node.kind === "symbol" ? undefined : renderSymbolEntry(node.node);
	}
}

function renderSection(label: string): DetailDocument {
	return {
		title: label,
		kind: "Workspace",
		description: "Select a row inside this section to inspect its details.",
	};
}

function renderDaemon(node: DaemonNode): DetailDocument {
	return {
		title: node.entry.workspace_root,
		kind: node.current ? "Current daemon" : "Daemon",
		meta: rows({
			endpoint: node.entry.endpoint,
			pid: String(node.entry.pid),
			refresh: node.entry.live_refresh ?? "unknown",
			roots: node.entry.workspace_roots.join(", "),
		}),
	};
}

function renderViewNode(node: ViewTreeNode): DetailDocument {
	switch (node.kind) {
		case "view":
			return {
				title: node.view.title ?? node.view.id,
				kind: "View",
				meta: rows({
					id: node.view.id,
					fragment: node.view.fragment,
					scope: node.view.scope,
					anchor: node.view.anchor,
				}),
			};
		case "section":
			return {
				title: node.label,
				kind: "View section",
				description: node.view.summary ?? node.view.intent ?? undefined,
				meta: rows({
					view: node.view.id,
					fragment: node.view.fragment,
					scope: node.view.scope,
				}),
			};
		case "boundary":
			return {
				title: node.boundary.id,
				kind: "Boundary",
				description: node.boundary.rationale ?? undefined,
				sections: [
					{ title: "Owns", rows: valueRows(node.boundary.owns) },
					{ title: "Forbids", rows: valueRows(node.boundary.forbids) },
					{ title: "Missing", rows: valueRows(node.boundary.missing) },
				],
			};
		case "gotcha":
			return {
				title: node.gotcha.id,
				kind: "Gotcha",
				description: node.gotcha.rationale,
				meta: rows({ check: node.gotcha.check ?? "" }),
				sections: [{ title: "Missing", rows: valueRows(node.gotcha.missing) }],
			};
		case "evidence":
			return {
				title: node.evidence.label,
				kind: "Evidence",
				meta: rows({
					selector: node.evidence.selector,
					moniker: node.evidence.moniker,
					file: node.evidence.file,
					slice: rangeLabel(node.evidence.slice),
				}),
			};
		case "rule":
			return {
				title: node.rule.id,
				kind: "View rule",
				description: node.rule.rationale ?? undefined,
				meta: rows({ severity: node.rule.severity, domain: node.rule.domain }),
			};
		default:
			return { title: node.label, kind: "Info" };
	}
}

function renderCheckNode(node: RulesTreeNode): DetailDocument {
	switch (node.kind) {
		case "section":
			return { title: node.label, kind: "Check", description: "Latest daemon check findings." };
		case "rule":
			return {
				title: node.rule.id,
				kind: "Rule",
				description: node.rule.rationale ?? node.rule.message ?? undefined,
				meta: rows({
					severity: node.rule.severity,
					language: node.rule.lang,
					domain: node.rule.domain,
				}),
				sections: [{ title: "Expression", text: node.rule.expr }],
			};
		case "group":
			return {
				title: node.file,
				kind: "Finding group",
				meta: rows({ root: node.root, violations: String(node.violations.length) }),
			};
		case "violation":
			return {
				title: node.violation.rule_id,
				kind: "Violation",
				description: node.violation.message,
				meta: rows({
					severity: node.violation.severity,
					file: node.violation.path,
					lines: rangeLabel(node.violation.lines),
					moniker: node.violation.moniker,
				}),
			};
		default:
			return { title: node.label, kind: "Info" };
	}
}

function renderRuleFileNode(node: RuleTreeNode): DetailDocument {
	switch (node.kind) {
		case "folder":
			return { title: node.label, kind: "Rule folder", meta: rows({ path: node.relativePath }) };
		case "file":
			return {
				title: node.uri.fsPath,
				kind: "Rule file",
				meta: rows({
					rules: String(node.parsed.rules.length),
					aliases: String(node.parsed.aliases.length),
					profiles: String(node.parsed.profiles.length),
					fragment: node.parsed.fragment ?? "",
				}),
			};
		case "rule":
			return {
				title: node.rule.id,
				kind: "Rule definition",
				meta: rows({
					scope: node.rule.scope,
					severity: node.rule.severity,
					file: node.uri.fsPath,
				}),
				sections: [{ title: "Rule block", text: node.rule.blockText }],
			};
		default:
			return { title: node.label, kind: "Info" };
	}
}

function renderSymbolEntry(node: { kind: string; tree?: { path: string; defs: number; refs: number } }): DetailDocument {
	if (node.kind !== "entry" || !node.tree) {
		return { title: "Symbols", kind: "Info" };
	}
	return {
		title: node.tree.path,
		kind: "Source entry",
		meta: rows({
			defs: String(node.tree.defs),
			refs: String(node.tree.refs),
		}),
	};
}

function rows(values: Record<string, string | undefined>): DetailRow[] {
	return Object.entries(values)
		.filter(([, value]) => value !== undefined && value.length > 0)
		.map(([label, value]) => ({ label, value: value ?? "" }));
}

function valueRows(values: string[]): DetailRow[] {
	return values.length > 0
		? values.map((value, index) => ({ label: String(index + 1), value }))
		: [{ label: "none", value: "" }];
}

function rangeLabel(range: [number, number] | null | undefined): string {
	return range ? `${range[0]}-${range[1]}` : "";
}
