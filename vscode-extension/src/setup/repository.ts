import * as fs from "node:fs";
import * as path from "node:path";

import { agentStatus, cliVersion } from "../cli/facade";
import { RULES_FILE_NAME } from "../rules/repository";
import { AGENT_CLIENTS, AgentClient, AgentIntegration, SetupSnapshot, parseAgentStatus } from "./model";

// Reads the workspace's setup state through the CLI. Every probe is a process
// spawn, so the snapshot is cached until something invalidates it — the tree
// refreshes far more often than the configuration changes.
export class SetupRepository {
	private snapshot?: Promise<SetupSnapshot>;

	constructor(readonly root: string) {}

	get rulesPath(): string {
		return path.join(this.root, RULES_FILE_NAME);
	}

	load(): Promise<SetupSnapshot> {
		this.snapshot ??= this.probe();
		return this.snapshot;
	}

	invalidate(): void {
		this.snapshot = undefined;
	}

	// Re-reads only the rules row. Saving the rules file is frequent while
	// authoring; it cannot change an agent integration, so it must not cost
	// one CLI spawn per client.
	async refreshRules(): Promise<void> {
		const current = await this.load();
		this.snapshot = Promise.resolve({ ...current, rulesPresent: fs.existsSync(this.rulesPath) });
	}

	private async probe(): Promise<SetupSnapshot> {
		const cli = await cliVersion();
		const rulesPresent = fs.existsSync(this.rulesPath);
		// Without a usable binary every agent probe would fail the same way,
		// after walking the candidate list again for each client.
		if (!cli.ok) {
			return {
				cli: { error: cli.error },
				rulesPresent,
				integrations: AGENT_CLIENTS.map((client) => ({ client, components: [], error: cli.error })),
			};
		}
		return {
			cli: { version: cli.stdout.trim().split(/\s+/).pop() },
			rulesPresent,
			integrations: await Promise.all(AGENT_CLIENTS.map((client) => this.integration(client))),
		};
	}

	private async integration(client: AgentClient): Promise<AgentIntegration> {
		const result = await agentStatus(client, this.root);
		return result.ok
			? { client, components: parseAgentStatus(result.stdout) }
			: { client, components: [], error: result.error };
	}
}
