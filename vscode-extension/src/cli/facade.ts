import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { CliOutcome, missingBinaryMessage, runCli } from "./runner";
import { CheckReport, EvalReport } from "./model";

export interface EvalRequest {
	/** A real .code-moniker.toml fragment. */
	rulesToml: string;
	/** code-moniker language tag for the sample (rs, ts, …). */
	cliTag: string;
	/** Sample source, piped to the CLI on stdin. */
	source: string;
}

export type EvalResult =
	| { ok: true; report: EvalReport }
	| { ok: false; error: string };

// Writes the rule fragment to a temp file, runs `code-moniker rules eval` with
// the sample on stdin, and parses the JSON report.
export async function runEval(request: EvalRequest): Promise<EvalResult> {
	const dir = mkdtempSync(path.join(os.tmpdir(), "cmnb-"));
	const rulesPath = path.join(dir, "rules.toml");
	writeFileSync(rulesPath, request.rulesToml);
	try {
		const result = await runCli(
			["rules", "eval", "--rules", rulesPath, "--lang", request.cliTag, "--format", "json"],
			request.source,
		);
		return parseJson<EvalReport>(result);
	} finally {
		rmSync(dir, { recursive: true, force: true });
	}
}

export type CheckResult =
	| { ok: true; report: CheckReport }
	| { ok: false; error: string };

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
	return parseJson<CheckReport>(result);
}

export type ValidateResult = { ok: true } | { ok: false; error: string };

export async function validateRulesToml(
	root: string,
	rulesToml: string,
): Promise<ValidateResult> {
	const dir = mkdtempSync(path.join(os.tmpdir(), "cmnb-"));
	const rulesPath = path.join(dir, "rules.toml");
	writeFileSync(rulesPath, rulesToml);
	try {
		return validateRuleFile(root, rulesPath);
	} finally {
		rmSync(dir, { recursive: true, force: true });
	}
}

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

export interface LearnPackReport {
	samples: { name: string; content: string }[];
}

export type LearnPackResult =
	| { ok: true; report: LearnPackReport }
	| { ok: false; error: string };

export async function runLearnPack(name: string): Promise<LearnPackResult> {
	const result = await runCli(["rules", "learn", name, "--format", "json"]);
	return parseJson<LearnPackReport>(result);
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

function parseJson<T>(result: CliOutcome): { ok: true; report: T } | { ok: false; error: string } {
	const error = cliError(result);
	if (error || result.kind !== "done") {
		return { ok: false, error: error ?? "code-moniker did not run" };
	}
	try {
		return { ok: true, report: JSON.parse(result.stdout) as T };
	} catch (err) {
		return {
			ok: false,
			error: `Could not parse code-moniker output: ${(err as Error).message}\n${result.stdout}`,
		};
	}
}
