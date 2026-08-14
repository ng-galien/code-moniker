import { registerCatalog } from "./catalog/catalogView";
import { registerAcceptanceControl } from "./acceptanceControl";
import { ExplorerFeature, registerExplorer } from "./explorer/manager";
import { registerChanges } from "./changes/manager";
import { ChangesProvider } from "./changes/tree";
import { DaemonListProvider } from "./daemon/tree";
import { DaemonSession } from "./daemon/session";
import { registerDaemon } from "./daemon/manager";
import { registerRuleManager } from "./rules/manager";
import { registerRulesDaemon } from "./rules-daemon/manager";
import { RulesProvider } from "./rules-daemon/tree";
import { ViolationModel } from "./rules-daemon/decorations";
import { registerScenario } from "./scenario/manager";
import { registerSetup } from "./setup/manager";
import { SetupProvider } from "./setup/tree";
import { registerSymbols } from "./symbols/manager";
import { DetailWebview } from "./symbols/detail/panel";
import { SymbolTreeProvider } from "./symbols/tree";
import { registerViews } from "./views/manager";
import { registerWorkspace } from "./workbench/manager";
import { WorkspaceTreeProvider } from "./workbench/workspaceTree";
import * as vscode from "vscode";

// Surface the feature internals so the e2e acceptance suite can drive and inspect
// the daemon-backed views without scraping the UI.
export interface CodeMonikerApi {
	session: DaemonSession;
	daemons: DaemonListProvider;
	symbols: SymbolTreeProvider;
	detail: DetailWebview;
	rules: RulesProvider;
	changes: ChangesProvider;
	violations: ViolationModel;
	workspace: WorkspaceTreeProvider;
	workspaceSync: { revealRequests: number; revealOperations: number };
	setup: SetupProvider;
	explorer: ExplorerFeature;
}

let activeDaemonSession: DaemonSession | undefined;

export function activate(context: vscode.ExtensionContext): CodeMonikerApi {
	const acceptanceControl = registerAcceptanceControl(context);
	if (acceptanceControl) context.subscriptions.push(acceptanceControl);
	const ruleFiles = registerRuleManager(context);
	registerCatalog(context);
	registerScenario(context);

	const daemon = registerDaemon(context);
	activeDaemonSession = daemon.session;
	const symbols = registerSymbols(context, daemon.session);
	const views = registerViews(context, daemon.session);
	const rules = registerRulesDaemon(context, daemon.session, symbols);
	const changes = registerChanges(context, daemon.session);
	const setup = registerSetup(context);
	const workspace = registerWorkspace(context, {
		session: daemon.session,
		setup: setup.provider,
		daemons: daemon.provider,
		symbols: symbols.tree,
		views: views.provider,
		changes: changes.provider,
		detail: symbols.detail,
		rules: rules.provider,
		ruleFiles: ruleFiles.provider,
	});
	const explorer = registerExplorer(context, daemon.session, symbols.repository, workspace);

	const api = {
		session: daemon.session,
		daemons: daemon.provider,
		symbols: symbols.tree,
		detail: symbols.detail,
		rules: rules.provider,
		changes: changes.provider,
		violations: rules.model,
		workspace: workspace.tree,
		workspaceSync: workspace.syncStats,
		setup: setup.provider,
		explorer,
	};
	acceptanceControl?.markReady();
	return api;
}

export async function deactivate(): Promise<void> {
	const session = activeDaemonSession;
	activeDaemonSession = undefined;
	await session?.shutdownOwned();
}
