import * as vscode from "vscode";

import { RULES_FILE_NAME } from "../rules/repository";
import { watchAndRefresh } from "../shared/watch";
import { primaryWorkspaceRoot } from "../shared/workspace";
import { registerSetupCommands } from "./commands";
import { SetupRepository } from "./repository";
import { SetupProvider } from "./tree";

export interface SetupFeature {
	provider: SetupProvider;
}

// Where each client keeps the project-scoped files `agent install` writes, so
// an integration installed from a terminal shows up without a manual refresh.
const CLIENT_CONFIG_GLOB =
	"{.mcp.json,.claude/settings.json,.claude/hooks/**,.codex/config.toml,.codex/hooks.json,.gemini/settings.json}";

// Boots the Setup rows: one repository for the workspace root, a provider for
// the tree, the commands that fix each row, and watchers split by cause — a
// rules-file save must not respawn one CLI process per agent client.
export function registerSetup(context: vscode.ExtensionContext): SetupFeature {
	const root = primaryWorkspaceRoot();
	const repository = new SetupRepository(root);
	const provider = new SetupProvider(repository);

	registerSetupCommands(context, repository, provider);

	context.subscriptions.push(
		...watchAndRefresh(new vscode.RelativePattern(root, RULES_FILE_NAME), () =>
			void provider.refreshRules(),
		),
		...watchAndRefresh(new vscode.RelativePattern(root, CLIENT_CONFIG_GLOB), () =>
			provider.refresh(),
		),
	);

	return { provider };
}
