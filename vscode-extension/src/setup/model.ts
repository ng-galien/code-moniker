import { SetupStatus } from "../shared/appIcons";

// What an unconfigured workspace is missing. The CLI owns the truth about
// managed integrations, so the extension reads `agent status` rather than
// guessing from files on disk.

export const AGENT_CLIENTS = ["claude", "codex", "gemini"] as const;

export type AgentClient = (typeof AGENT_CLIENTS)[number];

export const CLIENT_LABELS: Record<AgentClient, string> = {
	claude: "Claude Code",
	codex: "Codex",
	gemini: "Gemini CLI",
};

// Installing without naming components gets skill + mcp only; the hook is
// what makes the agent check its own edits, so a click installs all three.
export const FULL_COMPONENTS = "skill,mcp,hooks";

// One row of `code-moniker agent status`: a component the CLI installed and
// still tracks, with where it landed. `state` is `missing` once the file is
// gone, which is how a half-broken integration shows without the doctor.
export interface AgentComponent {
	component: string;
	scope: string;
	state: string;
	version: string;
	location: string;
}

export interface AgentIntegration {
	client: AgentClient;
	components: AgentComponent[];
	// Set when the CLI could not answer at all (missing binary, spawn error);
	// distinct from "answered, nothing installed".
	error?: string;
}

export interface CliState {
	version?: string;
	error?: string;
}

export interface SetupSnapshot {
	cli: CliState;
	rulesPresent: boolean;
	integrations: AgentIntegration[];
}

// Row health, shared with the icon vocabulary so no translation sits between
// the state and the colour it earns.
export function integrationHealth(integration: AgentIntegration): SetupStatus {
	if (integration.error !== undefined) {
		return "error";
	}
	if (integration.components.length === 0) {
		return "absent";
	}
	return integration.components.some((component) => component.state === "missing")
		? "missing"
		: "ok";
}

// The status table is fixed-width with at least two spaces between columns;
// anything else (an empty listing, a "No managed …" sentence) yields no rows.
export function parseAgentStatus(stdout: string): AgentComponent[] {
	const rows: AgentComponent[] = [];
	for (const line of stdout.split("\n")) {
		const cells = line.trim().split(/\s{2,}/);
		if (cells.length < 6 || cells[0] === "client") {
			continue;
		}
		rows.push({
			component: cells[1],
			scope: cells[2],
			state: cells[3],
			version: cells[4],
			location: cells.slice(5).join("  "),
		});
	}
	return rows;
}
