import { ChangeTreeNode } from "../changes/nodes";
import { RulesTreeNode } from "../rules-daemon/nodes";
import { SymbolTreeNode } from "../symbols/nodes";
import { ViewTreeNode } from "../views/nodes";
import { WorkspaceNode } from "./workspaceTree";

// Stable TreeItem ids for the composite workspace tree. VS Code matches nodes
// across refreshes by id, so expansion and selection survive the event-driven
// refresh cascade instead of collapsing on every daemon event. Ids must be
// unique tree-wide; each branch is namespaced by its section.
export function workspaceNodeId(node: WorkspaceNode): string | undefined {
	switch (node.kind) {
		case "section":
			return `section:${node.id}`;
		case "daemon":
			return `daemon:${node.node.entry.endpoint}`;
		case "symbols":
			return symbolNodeId(node.node);
		case "views":
			return viewNodeId(node.node);
		case "changes":
			return changeNodeId(node.node);
		case "check":
			return rulesNodeId(node.node);
		case "ruleFiles":
			return undefined;
	}
}

function symbolNodeId(node: SymbolTreeNode): string {
	switch (node.kind) {
		case "identity":
			return `symbols:id:${node.row.identity}`;
		case "symbol":
			return `symbols:sym:${node.symbol.file}:${node.symbol.uri}`;
		case "info":
			return `symbols:info:${node.label}`;
	}
}

function viewNodeId(node: ViewTreeNode): string {
	switch (node.kind) {
		case "view":
			return `views:view:${node.view.id}`;
		case "section":
			return `views:section:${node.view.id}:${node.id}`;
		case "boundary":
			return `views:boundary:${node.boundary.id}`;
		case "gotcha":
			return `views:gotcha:${node.gotcha.id}`;
		case "evidence":
			return `views:evidence:${node.evidence.file}:${node.evidence.label}`;
		case "rule":
			return `views:rule:${node.rule.id}`;
		case "info":
			return `views:info:${node.label}`;
	}
}

function changeNodeId(node: ChangeTreeNode): string {
	switch (node.kind) {
		case "file":
			return `changes:file:${node.file.old_path ?? ""}:${node.file.new_path ?? ""}`;
		case "symbolChange":
			return `changes:sym:${node.change.kind}:${node.change.old?.identity ?? ""}:${node.change.new?.identity ?? ""}`;
		case "info":
			return `changes:info:${node.label}`;
	}
}

function rulesNodeId(node: RulesTreeNode): string {
	switch (node.kind) {
		case "section":
			return `check:section:${node.id}`;
		case "rule":
			return `check:rule:${node.rule.id}`;
		case "group":
			return `check:group:${node.root}:${node.file}`;
		case "violation": {
			const violation = node.violation;
			return `check:violation:${violation.rule_id}:${violation.path}:${violation.lines[0]}:${violation.moniker}`;
		}
		case "info":
			return `check:info:${node.label}`;
	}
}
