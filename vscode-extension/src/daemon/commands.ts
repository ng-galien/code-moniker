import * as vscode from "vscode";

import { DaemonNode } from "./nodes";
import { DaemonRpc } from "./rpc";
import { DaemonListProvider } from "./tree";
import { DaemonSession } from "./session";

export function registerDaemonCommands(
	context: vscode.ExtensionContext,
	session: DaemonSession,
	provider: DaemonListProvider,
): void {
	context.subscriptions.push(
		vscode.commands.registerCommand("codeMoniker.daemon.refresh", () => provider.refresh()),
		vscode.commands.registerCommand("codeMoniker.daemon.connect", () => connect(session, provider)),
		vscode.commands.registerCommand("codeMoniker.daemon.stop", (node?: DaemonNode) =>
			stop(session, provider, node),
		),
		vscode.commands.registerCommand("codeMoniker.daemon.openWorkspace", (node?: DaemonNode) =>
			openWorkspace(node),
		),
	);
}

async function connect(session: DaemonSession, provider: DaemonListProvider): Promise<void> {
	const ok = await vscode.window.withProgress(
		{ location: { viewId: "codeMoniker.daemons" }, title: "Connecting to daemon…" },
		() => session.connectOrStart(),
	);
	provider.refresh();
	if (!ok && session.lastError) {
		void vscode.window.showErrorMessage(`code-moniker daemon: ${session.lastError}`);
	}
}

async function stop(
	session: DaemonSession,
	provider: DaemonListProvider,
	node?: DaemonNode,
): Promise<void> {
	if (!node || node.current) {
		await session.stop();
		provider.refresh();
		return;
	}
	try {
		const rpc = await DaemonRpc.connect(node.entry.endpoint);
		await rpc.shutdown();
		rpc.close();
	} catch (error) {
		void vscode.window.showErrorMessage(`Could not stop daemon: ${(error as Error).message}`);
	}
	provider.refresh();
}

async function openWorkspace(node?: DaemonNode): Promise<void> {
	if (!node) {
		return;
	}
	await vscode.commands.executeCommand(
		"vscode.openFolder",
		vscode.Uri.file(node.entry.workspace_root),
		{ forceNewWindow: true },
	);
}
