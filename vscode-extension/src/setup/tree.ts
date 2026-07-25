import * as vscode from "vscode";

import { SetupStatus, setupComponentIcon, setupStatusIcon } from "../shared/appIcons";
import { CLIENT_LABELS, integrationHealth } from "./model";
import { SetupAgentNode, SetupCliNode, SetupComponentNode, SetupRulesNode, SetupTreeNode } from "./nodes";
import { SetupRepository } from "./repository";

// The Setup rows of the workspace tree: what a freshly opened folder has, and
// what it is still missing. Every row that can be fixed carries the command
// that fixes it, so nothing here needs the terminal.
export class SetupProvider implements vscode.TreeDataProvider<SetupTreeNode> {
	private readonly emitter = new vscode.EventEmitter<SetupTreeNode | undefined>();
	readonly onDidChangeTreeData = this.emitter.event;

	constructor(private readonly repository: SetupRepository) {}

	refresh(): void {
		this.repository.invalidate();
		this.emitter.fire(undefined);
	}

	// Re-reads the rules row alone, for the events that cannot have touched
	// an agent integration.
	async refreshRules(): Promise<void> {
		await this.repository.refreshRules();
		this.emitter.fire(undefined);
	}

	// Only the root listing needs the snapshot; expanding an agent row reads
	// what that row already carries, and must not pay for a re-probe.
	async getChildren(node?: SetupTreeNode): Promise<SetupTreeNode[]> {
		if (!node) {
			const snapshot = await this.repository.load();
			const rows: SetupTreeNode[] = [
				{ kind: "cli", cli: snapshot.cli },
				{ kind: "rules", present: snapshot.rulesPresent },
			];
			for (const integration of snapshot.integrations) {
				rows.push({ kind: "agent", integration });
			}
			return rows;
		}
		if (node.kind === "agent") {
			return node.integration.components.map((component) => ({
				kind: "component",
				client: node.integration.client,
				component,
			}));
		}
		return [];
	}

	getTreeItem(node: SetupTreeNode): vscode.TreeItem {
		switch (node.kind) {
			case "cli":
				return cliItem(node);
			case "rules":
				return rulesItem(node, this.repository.rulesPath);
			case "agent":
				return agentItem(node);
			case "component":
				return componentItem(node);
		}
	}
}

function cliItem(node: SetupCliNode): vscode.TreeItem {
	const { version, error } = node.cli;
	const item = new vscode.TreeItem("CLI", vscode.TreeItemCollapsibleState.None);
	item.description = version ?? "not found";
	item.iconPath = setupStatusIcon(version ? "ok" : "error");
	item.tooltip = error ?? `code-moniker ${version} — the binary backing every command`;
	item.contextValue = "cmSetupCli";
	return item;
}

function rulesItem(node: SetupRulesNode, rulesPath: string): vscode.TreeItem {
	const item = new vscode.TreeItem("Rules file", vscode.TreeItemCollapsibleState.None);
	item.description = node.present ? ".code-moniker.toml" : "not initialized";
	item.iconPath = setupStatusIcon(node.present ? "ok" : "missing");
	item.tooltip = node.present
		? rulesPath
		: "No .code-moniker.toml — run “Initialize Rules File” to create one with detected aliases.";
	item.contextValue = node.present ? "cmSetupRulesPresent" : "cmSetupRulesMissing";
	item.command = node.present
		? { command: "vscode.open", title: "Open", arguments: [vscode.Uri.file(rulesPath)] }
		: { command: "codeMoniker.setup.initRules", title: "Initialize Rules File" };
	return item;
}

interface AgentRow {
	description: string;
	tooltip: string;
	context: string;
	expandable: boolean;
}

function agentItem(node: SetupAgentNode): vscode.TreeItem {
	const health = integrationHealth(node.integration);
	const row = agentRow(node, health);
	const item = new vscode.TreeItem(
		CLIENT_LABELS[node.integration.client],
		row.expandable
			? vscode.TreeItemCollapsibleState.Collapsed
			: vscode.TreeItemCollapsibleState.None,
	);
	item.description = row.description;
	item.tooltip = row.tooltip;
	item.iconPath = setupStatusIcon(health);
	item.contextValue = row.context;
	return item;
}

// One place decides everything the row says, so the four properties cannot
// drift out of agreement with the health they describe.
function agentRow(node: SetupAgentNode, health: SetupStatus): AgentRow {
	const { integration } = node;
	const label = CLIENT_LABELS[integration.client];
	switch (health) {
		case "error":
			return {
				description: "unavailable",
				tooltip: integration.error ?? "",
				context: "cmSetupAgentMissing",
				expandable: false,
			};
		case "missing": {
			const gone = integration.components
				.filter((component) => component.state === "missing")
				.map((component) => component.component);
			return {
				description: `${gone.join(", ")} missing`,
				tooltip: `${label}: ${gone.join(", ")} no longer on disk — right-click to repair or diagnose.`,
				context: "cmSetupAgentBroken",
				expandable: true,
			};
		}
		case "ok":
			return {
				description: integration.components.map((component) => component.component).join(" · "),
				tooltip: `${label} integration installed — right-click to update, diagnose or remove.`,
				context: "cmSetupAgentInstalled",
				expandable: true,
			};
		case "absent":
			return {
				description: "not installed",
				tooltip: `No managed ${label} integration — right-click to install the skill, MCP server and check hook.`,
				context: "cmSetupAgentMissing",
				expandable: false,
			};
	}
}

function componentItem(node: SetupComponentNode): vscode.TreeItem {
	const { component, scope, state, version, location } = node.component;
	const gone = state === "missing";
	const item = new vscode.TreeItem(component, vscode.TreeItemCollapsibleState.None);
	item.description = `${scope} · ${state} · ${version}`;
	// A tracked component that vanished from disk is the failure the doctor
	// would report: it earns the alert icon, not its own component glyph.
	item.iconPath = gone ? setupStatusIcon("missing") : setupComponentIcon(component);
	item.tooltip = gone ? `${location} — no longer on disk` : location;
	item.contextValue = "cmSetupComponent";
	if (location.startsWith("/")) {
		item.resourceUri = vscode.Uri.file(location);
	}
	return item;
}
