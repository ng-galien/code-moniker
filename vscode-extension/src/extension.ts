import { registerCatalog } from "./catalog/catalogView";
import { DaemonListProvider } from "./daemon/tree";
import { DaemonSession } from "./daemon/session";
import { registerDaemon } from "./daemon/manager";
import { registerRuleManager } from "./rules/manager";
import { registerRulesDaemon } from "./rules-daemon/manager";
import { RulesProvider } from "./rules-daemon/tree";
import { ViolationModel } from "./rules-daemon/decorations";
import { registerScenario } from "./scenario/manager";
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
	violations: ViolationModel;
	workspace: WorkspaceTreeProvider;
}

export function activate(context: vscode.ExtensionContext): CodeMonikerApi {
	const ruleFiles = registerRuleManager(context);
	registerCatalog(context);
	registerScenario(context);

	const daemon = registerDaemon(context);
	const symbols = registerSymbols(context, daemon.session);
	const views = registerViews(context, daemon.session);
	const rules = registerRulesDaemon(context, daemon.session, symbols);
	const workspace = registerWorkspace(context, {
		session: daemon.session,
		daemons: daemon.provider,
		symbols: symbols.tree,
		views: views.provider,
		detail: symbols.detail,
		rules: rules.provider,
		ruleFiles: ruleFiles.provider,
	});

	return {
		session: daemon.session,
		daemons: daemon.provider,
		symbols: symbols.tree,
		detail: symbols.detail,
		rules: rules.provider,
		violations: rules.model,
		workspace: workspace.tree,
	};
}

export function deactivate(): void {
}
