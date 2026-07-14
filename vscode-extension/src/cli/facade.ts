import { CliOutcome, launchDetached, missingBinaryMessage, runCli } from "./runner";
import { CheckReport } from "./model";

export type CheckResult =
	| { ok: true; report: CheckReport }
	| { ok: false; error: string };

export async function launchWorkspaceDaemon(root: string): Promise<void> {
	const probe = await runCli(["--version"]);
	const error = cliError(probe);
	if (error) {
		throw new Error(error);
	}
	launchDetached(["daemon", "start", root]);
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

export type ValidateResult = { ok: true } | { ok: false; error: string };

// Validates a rules file by compiling it through `rules show`.
export async function validateRuleFile(
	root: string,
	rulesPath: string,
): Promise<ValidateResult> {
	const result = await runCli([
		"rules",
		"show",
		root,
		"--rules",
		rulesPath,
		"--format",
		"json",
	]);
	const error = cliError(result);
	return error ? { ok: false, error } : { ok: true };
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
