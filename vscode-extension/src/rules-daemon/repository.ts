import { RuleDto, RulesCheckResult, ViolationDto } from "../daemon/model";
import { DaemonSession } from "../daemon/session";

// Rules listing and check execution over the shared session.
export class RulesRepository {
	constructor(private readonly session: DaemonSession) {}

	get ready(): boolean {
		return this.session.ready;
	}

	async listRules(): Promise<RuleDto[]> {
		const response = await this.session.query({
			op: "rules_list",
			workspace: null,
			profile: null,
			rules: null,
			lang: [],
			severity: [],
		});
		return response.result.kind === "rules_list" ? response.result.data.rows : [];
	}

	async runCheck(): Promise<{ summary: RulesCheckResult["summary"]; violations: ViolationDto[] }> {
		const response = await this.session.query({
			op: "rules_check",
			workspace: null,
			profile: null,
			rules: null,
			file: [],
			report: true,
		});
		if (response.result.kind !== "rules_check") {
			return { summary: emptySummary(), violations: [] };
		}
		return { summary: response.result.data.summary, violations: response.result.data.violations };
	}
}

function emptySummary(): RulesCheckResult["summary"] {
	return {
		files_scanned: 0,
		files_with_violations: 0,
		total_violations: 0,
		total_rule_errors: 0,
		total_warnings: 0,
		files_with_errors: 0,
		total_errors: 0,
		elapsed_ms: 0,
	};
}
