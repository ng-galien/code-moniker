import * as path from "node:path";
import * as vscode from "vscode";

import { CheckSummaryDto, RuleDto, ViolationDto } from "../daemon/model";
import { toRelative } from "../daemon/paths";
import { DaemonSession } from "../daemon/session";
import { GroupNode, RuleNode, RulesTreeNode, SectionNode, ViolationNode } from "./nodes";
import { RulesRepository } from "./repository";

// A two-section tree: the active rule set (rules.list) and the latest check run
// (rules.check) with violations grouped by file.
export class RulesProvider implements vscode.TreeDataProvider<RulesTreeNode> {
	private readonly emitter = new vscode.EventEmitter<RulesTreeNode | undefined>();
	readonly onDidChangeTreeData = this.emitter.event;

	private rulesCache?: RuleDto[];
	private summary?: CheckSummaryDto;
	private violations: ViolationDto[] = [];

	constructor(
		private readonly session: DaemonSession,
		private readonly repository: RulesRepository,
	) {}

	refresh(): void {
		this.rulesCache = undefined;
		this.emitter.fire(undefined);
	}

	setCheck(summary: CheckSummaryDto, violations: ViolationDto[]): void {
		this.summary = summary;
		this.violations = violations;
		this.emitter.fire(undefined);
	}

	async getChildren(node?: RulesTreeNode): Promise<RulesTreeNode[]> {
		if (!node) {
			return [
				{ kind: "section", id: "rules", label: "Rules" },
				{ kind: "section", id: "check", label: "Check" },
			];
		}
		if (node.kind === "section" && node.id === "rules") {
			return this.ruleChildren();
		}
		if (node.kind === "section" && node.id === "check") {
			return this.checkChildren();
		}
		if (node.kind === "group") {
			return node.violations.map((violation) => ({ kind: "violation", violation }));
		}
		return [];
	}

	getTreeItem(node: RulesTreeNode): vscode.TreeItem {
		switch (node.kind) {
			case "section":
				return this.sectionItem(node);
			case "rule":
				return ruleItem(node);
			case "group":
				return groupItem(node);
			case "violation":
				return violationItem(node);
			default:
				return new vscode.TreeItem(node.label);
		}
	}

	private async ruleChildren(): Promise<RulesTreeNode[]> {
		if (!this.session.ready) {
			return [{ kind: "info", label: "Daemon not ready" }];
		}
		if (!this.rulesCache) {
			this.rulesCache = await this.repository.listRules();
		}
		if (this.rulesCache.length === 0) {
			return [{ kind: "info", label: "No rules in the active profile" }];
		}
		return this.rulesCache.map((rule) => ({ kind: "rule", rule }));
	}

	private checkChildren(): RulesTreeNode[] {
		if (!this.summary) {
			return [{ kind: "info", label: "Run check to populate findings" }];
		}
		if (this.violations.length === 0) {
			return [{ kind: "info", label: "No violations" }];
		}
		const groups = new Map<string, GroupNode>();
		for (const violation of this.violations) {
			const relPath = toRelative(violation.root, violation.path);
			const key = `${violation.root}\0${relPath}`;
			const group = groups.get(key);
			if (group) {
				group.violations.push(violation);
			} else {
				groups.set(key, {
					kind: "group",
					root: violation.root,
					file: relPath,
					violations: [violation],
				});
			}
		}
		return [...groups.values()].sort((a, b) => a.file.localeCompare(b.file));
	}

	private sectionItem(node: SectionNode): vscode.TreeItem {
		const item = new vscode.TreeItem(node.label, vscode.TreeItemCollapsibleState.Expanded);
		item.contextValue = node.id === "rules" ? "cmRulesSection" : "cmCheckSection";
		if (node.id === "rules") {
			item.iconPath = new vscode.ThemeIcon("law");
		} else {
			item.iconPath = new vscode.ThemeIcon("checklist");
			item.description = this.checkSummaryLabel();
		}
		return item;
	}

	private checkSummaryLabel(): string {
		if (!this.summary) {
			return "not run";
		}
		const { total_violations, files_scanned } = this.summary;
		return total_violations === 0
			? `clean · ${files_scanned} files`
			: `${total_violations} violation(s)`;
	}
}

function ruleItem(node: RuleNode): vscode.TreeItem {
	const rule = node.rule;
	const item = new vscode.TreeItem(rule.id, vscode.TreeItemCollapsibleState.None);
	item.description = [rule.severity, rule.lang, rule.domain].filter(Boolean).join(" · ");
	item.iconPath = rule.severity === "warn"
		? warnIcon()
		: new vscode.ThemeIcon("shield", new vscode.ThemeColor("charts.blue"));
	item.contextValue = "cmDaemonRule";
	item.tooltip = ruleTooltip(rule);
	return item;
}

function ruleTooltip(rule: RuleDto): vscode.MarkdownString {
	const md = new vscode.MarkdownString();
	md.appendMarkdown(`**${rule.id}** · ${rule.severity}\n\n`);
	if (rule.message) {
		md.appendMarkdown(`${rule.message}\n\n`);
	}
	md.appendCodeblock(rule.expr, "text");
	if (rule.rationale) {
		md.appendMarkdown(`\n${rule.rationale}`);
	}
	return md;
}

function groupItem(node: GroupNode): vscode.TreeItem {
	const item = new vscode.TreeItem(
		path.basename(node.file) || node.file,
		vscode.TreeItemCollapsibleState.Collapsed,
	);
	item.description = `${node.violations.length} · ${path.dirname(node.file)}`;
	item.iconPath = warnIcon();
	item.resourceUri = vscode.Uri.file(path.join(node.root, node.file));
	item.contextValue = "cmCheckGroup";
	return item;
}

function violationItem(node: ViolationNode): vscode.TreeItem {
	const violation = node.violation;
	const item = new vscode.TreeItem(violation.rule_id, vscode.TreeItemCollapsibleState.None);
	item.description = `L${violation.lines[0]} · ${violation.message}`;
	item.iconPath = violation.severity === "warn"
		? warnIcon()
		: new vscode.ThemeIcon("error", new vscode.ThemeColor("errorForeground"));
	item.contextValue = "cmViolation";
	item.tooltip = `${violation.moniker}\n${violation.message}`;
	item.command = {
		command: "codeMoniker.symbols.openSource",
		title: "Open",
		arguments: [{ root: violation.root, file: violation.path, line: violation.lines[0] }],
	};
	return item;
}

function warnIcon(): vscode.ThemeIcon {
	return new vscode.ThemeIcon("warning", new vscode.ThemeColor("list.warningForeground"));
}
