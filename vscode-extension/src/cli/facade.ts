import {
	CliOutcome,
	DetachedProcess,
	binaryCandidates,
	launchDetached,
	missingBinaryMessage,
	runCli,
} from "./runner";
import { CheckReport } from "./model";

export type CheckResult =
	| { ok: true; report: CheckReport }
	| { ok: false; error: string };

export async function launchWorkspaceDaemon(roots: string[]): Promise<DetachedProcess> {
	const probe = await runCli(["--version"]);
	const error = cliError(probe);
	if (error) {
		throw new Error(error);
	}
	return launchDetached([
		"daemon",
		"start",
		...roots,
		"--supervisor-pid",
		String(process.pid),
		"--supervisor-fd",
		"3",
	]);
}

// Runs `code-moniker check <root> --rules <file> [--profile p]` over the project.
export async function runCheckProject(
	root: string,
	rulesPath: string,
	profile?: string,
): Promise<CheckResult> {
	const args = ["check", root, "--rules", rulesPath, "--format", "json"];
	if (profile) {
		args.push("--profile", profile);
	}
	const result = await runCli(args);
	return parseCheckJson(result);
}

export interface ScenarioCheckRequest {
	document: string;
	targetFile?: string;
}

export type ScenarioCheckResult =
	| { ok: true; report: CheckReport; target: string }
	| { ok: false; error: string };

export async function runScenarioCheck(request: ScenarioCheckRequest): Promise<ScenarioCheckResult> {
	const args = ["check", ".", "--scenario", "-", "--format", "json"];
	if (request.targetFile) {
		args.push("--file", request.targetFile);
	}
	const result = await runCli(args, request.document);
	const parsed = parseCheckJson(result);
	return parsed.ok
		? { ok: true, report: parsed.report, target: request.targetFile ?? "." }
		: parsed;
}

// A CLI run reduced to its output or the binary's own failure message.
export type CliText = { ok: true; stdout: string } | { ok: false; error: string };

// Validates a rules file by compiling it through `rules show`.
export async function validateRuleFile(root: string, rulesPath: string): Promise<CliText> {
	return text(await runCli(["rules", "show", root, "--rules", rulesPath, "--format", "json"]));
}

// Setup surface: the CLI owns what "initialized" means for a workspace, so
// the extension asks it rather than inspecting files.
export type AgentAction = "install" | "update" | "uninstall";

// A binary answers the same version for as long as it stays the same binary,
// so the probe runs once instead of on every Setup snapshot and every daemon
// relaunch. Keying on the resolved candidates makes a changed
// `codeMoniker.binaryPath` miss the memo on its own — no invalidation hook,
// and no reason for a registrar to reach into the facade.
let versionProbe: { key: string; result: Promise<CliText> } | undefined;

export function cliVersion(): Promise<CliText> {
	const key = binaryCandidates().join("|");
	if (versionProbe?.key !== key) {
		versionProbe = { key, result: runCli(["--version"]).then(text) };
	}
	return versionProbe.result;
}

export async function agentStatus(client: string, root: string): Promise<CliText> {
	return text(await runCli(["agent", "status", "--client", client, root]));
}

export async function runAgentAction(
	action: AgentAction,
	client: string,
	root: string,
	components?: string,
): Promise<CliText> {
	const args = ["agent", action, "--client", client];
	if (components) {
		args.push("--components", components);
	}
	args.push(root);
	return text(await runCli(args));
}

// `agent doctor` exits 1 when it finds problems and prints them — with the
// repair command — on stdout. That is a result, not a failure, so it gets its
// own shape instead of collapsing into CliText's error branch.
export interface DoctorReport {
	healthy: boolean;
	problems: string[];
	// The repair commands the CLI itself printed, as argv.
	repairs: string[][];
	error?: string;
}

export async function agentDoctor(client: string, root: string): Promise<DoctorReport> {
	const result = await runCli(["agent", "doctor", "--client", client, root]);
	if (result.kind !== "done") {
		return { healthy: false, problems: [], repairs: [], error: cliError(result) };
	}
	if (result.code === 0) {
		return { healthy: true, problems: [], repairs: [] };
	}
	const problems = tagged(result.stdout, "problem:");
	// The CLI computes the exact repair per component, carrying the rules
	// file, profile and check scope the integration was installed with.
	// Re-deriving it here would quietly reset that policy to defaults.
	const repairs = tagged(result.stdout, "fix:").map(cliArgv).filter((argv) => argv.length > 0);
	return {
		healthy: false,
		problems,
		repairs,
		error: problems.length > 0 ? undefined : cliError(result),
	};
}

function tagged(stdout: string, tag: string): string[] {
	return stdout
		.split("\n")
		.map((line) => line.trim())
		.filter((line) => line.startsWith(tag))
		.map((line) => line.slice(tag.length).trim());
}

// A `fix:` line is a `code-moniker …` invocation; run its arguments through
// the same resolver as every other call instead of a shell.
function cliArgv(command: string): string[] {
	const parts = command.split(/\s+/).filter(Boolean);
	return parts[0] === "code-moniker" ? parts.slice(1) : [];
}

export async function runCliArgs(args: string[]): Promise<CliText> {
	return text(await runCli(args));
}

export async function initRulesFile(root: string): Promise<CliText> {
	return text(await runCli(["rules", "init", root]));
}

function text(result: CliOutcome): CliText {
	const error = cliError(result);
	if (error) {
		return { ok: false, error };
	}
	return { ok: true, stdout: result.kind === "done" ? `${result.stdout}${result.stderr}` : "" };
}

// Maps any non-success CLI outcome to an error message, or undefined on success.
function cliError(result: CliOutcome): string | undefined {
	if (result.kind === "missing") {
		return missingBinaryMessage(result.tried);
	}
	if (result.kind === "spawnError") {
		return result.message;
	}
	if (result.code !== 0) {
		return result.stderr.trim() || `code-moniker exited with code ${result.code}`;
	}
	return undefined;
}

function parseCheckJson(result: CliOutcome): CheckResult {
	if (result.kind !== "done") {
		return { ok: false, error: cliError(result) ?? "code-moniker did not run" };
	}
	if (result.code > 1) {
		return { ok: false, error: result.stderr.trim() || `code-moniker exited with code ${result.code}` };
	}
	try {
		return { ok: true, report: JSON.parse(result.stdout) as CheckReport };
	} catch (err) {
		return {
			ok: false,
			error: `Invalid code-moniker JSON output: ${(err as Error).message}`,
		};
	}
}
