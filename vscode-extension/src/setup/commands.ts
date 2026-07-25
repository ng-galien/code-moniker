import * as vscode from "vscode";

import {
	AgentAction,
	CliText,
	agentDoctor,
	initRulesFile,
	runAgentAction,
	runCliArgs,
} from "../cli/facade";
import { unwrapWorkspaceNode } from "../shared/treeNodes";
import { firstLine } from "../shared/workspace";
import { AgentClient, CLIENT_LABELS, FULL_COMPONENTS } from "./model";
import { SetupTreeNode } from "./nodes";
import { SetupRepository } from "./repository";
import { SetupProvider } from "./tree";

const ACTION_LABELS: Record<AgentAction, string> = {
	install: "Installing",
	update: "Updating",
	uninstall: "Removing",
};

export function registerSetupCommands(
	context: vscode.ExtensionContext,
	repository: SetupRepository,
	provider: SetupProvider,
): void {
	const root = repository.root;

	const agentStep = async (action: AgentAction, node: unknown): Promise<void> => {
		const client = await resolveClient(node);
		if (!client) {
			return;
		}
		if (action === "uninstall" && !(await confirmRemoval(client))) {
			return;
		}
		// Install and update name every component: the default set omits the
		// check hook, and a half-installed integration is what the doctor
		// later complains about.
		const components = action === "uninstall" ? undefined : FULL_COMPONENTS;
		await apply(`${ACTION_LABELS[action]} ${CLIENT_LABELS[client]} integration…`, provider, () =>
			runAgentAction(action, client, root, components),
		);
	};

	for (const action of ["install", "update", "uninstall"] as const) {
		context.subscriptions.push(
			vscode.commands.registerCommand(`codeMoniker.setup.${action}Agent`, (node?: unknown) =>
				agentStep(action, node),
			),
		);
	}

	context.subscriptions.push(
		vscode.commands.registerCommand("codeMoniker.setup.refresh", () => provider.refresh()),
		vscode.commands.registerCommand("codeMoniker.setup.initRules", async () => {
			const result = await vscode.window.withProgress(
				{ location: { viewId: "codeMoniker.workspace" }, title: "Initializing rules file…" },
				() => initRulesFile(root),
			);
			// Only the rules row can have changed: refreshing the whole
			// snapshot would respawn one CLI process per agent client.
			await provider.refreshRules();
			if (!result.ok) {
				void vscode.window.showErrorMessage(`Code Moniker: ${firstLine(result.error)}`);
				return;
			}
			await vscode.window.showTextDocument(vscode.Uri.file(repository.rulesPath));
		}),
		vscode.commands.registerCommand("codeMoniker.setup.doctorAgent", (node?: unknown) =>
			diagnose(node, provider, root),
		),
	);
}

// `agent doctor` is read-only, so nothing is refreshed until a repair runs.
async function diagnose(node: unknown, provider: SetupProvider, root: string): Promise<void> {
	const client = await resolveClient(node);
	if (!client) {
		return;
	}
	const label = CLIENT_LABELS[client];
	const report = await agentDoctor(client, root);
	if (report.healthy) {
		void vscode.window.showInformationMessage(`Code Moniker — ${label}: integration is coherent.`);
		return;
	}
	if (report.problems.length === 0) {
		void vscode.window.showErrorMessage(
			`Code Moniker — ${label}: ${firstLine(report.error ?? "diagnosis failed")}`,
		);
		return;
	}
	if (report.repairs.length === 0) {
		void vscode.window.showWarningMessage(`Code Moniker — ${label}: ${report.problems.join("; ")}`);
		return;
	}
	// Run the CLI's own repair commands rather than a reconstruction: they
	// carry the rules file, profile and check scope this integration was
	// installed with, which a plain reinstall would reset to defaults.
	const repair = "Repair";
	const choice = await vscode.window.showWarningMessage(
		`Code Moniker — ${label}: ${report.problems.join("; ")}`,
		repair,
	);
	if (choice !== repair) {
		return;
	}
	await apply(`Repairing ${label} integration…`, provider, async () => {
		for (const argv of report.repairs) {
			const result = await runCliArgs(argv);
			if (!result.ok) {
				return result;
			}
		}
		return { ok: true, stdout: "" } as CliText;
	});
}

// Every mutating step always refreshes, including on failure: a partial
// install still changes what the tree should show.
async function apply(
	title: string,
	provider: SetupProvider,
	step: () => Promise<CliText>,
): Promise<void> {
	const result = await vscode.window.withProgress(
		{ location: { viewId: "codeMoniker.workspace" }, title },
		step,
	);
	provider.refresh();
	if (!result.ok) {
		void vscode.window.showErrorMessage(`Code Moniker: ${firstLine(result.error)}`);
	}
}

async function confirmRemoval(client: AgentClient): Promise<boolean> {
	const remove = "Remove";
	const choice = await vscode.window.showWarningMessage(
		`Remove the managed ${CLIENT_LABELS[client]} integration from this workspace?`,
		{ modal: true },
		remove,
	);
	return choice === remove;
}

// The commands are reachable both from a tree row and from the palette, where
// no node is passed and the client has to be picked.
async function resolveClient(node: unknown): Promise<AgentClient | undefined> {
	const fromNode = clientFromNode(node);
	if (fromNode) {
		return fromNode;
	}
	const picked = await vscode.window.showQuickPick(
		Object.entries(CLIENT_LABELS).map(([client, label]) => ({ label, client })),
		{ title: "Code Moniker agent integration" },
	);
	return picked?.client as AgentClient | undefined;
}

function clientFromNode(node: unknown): AgentClient | undefined {
	const setup = unwrapWorkspaceNode(node) as SetupTreeNode | undefined;
	if (setup?.kind === "agent") {
		return setup.integration.client;
	}
	if (setup?.kind === "component") {
		return setup.client;
	}
	return undefined;
}
