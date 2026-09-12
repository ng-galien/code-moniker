import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

export async function assertSqlIndexRules(client) {
	const inline = readFileSync(new URL("../../../samples/sql-indexes/rules.toml", import.meta.url), "utf8");
	const table = "CREATE TABLE app.child (a int, b int);";
	const fk = "ALTER TABLE app.child ADD CONSTRAINT child_fk FOREIGN KEY(a,b) REFERENCES app.parent(a,b);";
	const index = "CREATE INDEX child_idx ON app.child (a,b);";
	const statements = [table, fk, index];
	let revision = 0;
	async function check(contents, expected) {
		await client.sources.replace({
			srcset: "sql-index-rules", revision: String(++revision),
			documents: contents.map((content, i) => ({uri: `postgres://rules/statement-${i}.sql`, language: "sql", content})),
		});
		const response = await client.query({op: "rules_check", inline_rules: [inline], file: [], report: true}, {consistency: "stale_ok"});
		assert.equal(response.result.kind, "rules_check");
		assert.deepEqual(response.result.data.errors, []);
		const result = response.result.data;
		const id = "workspace.symbol.fk-index-prefix";
		assert.equal(result.violations.filter(v => v.rule_id === id).length, expected, `revision ${revision}`);
		const reports = result.rule_reports.filter(r => r.rule_id === id);
		assert(reports.some(r => r.antecedent_matches === 1), "the FK must be evaluated");
		assert(reports.every(r => r.inconclusive === 0), "the relation must resolve");
	}
	for (const order of [[0,1,2],[0,2,1],[1,0,2],[1,2,0],[2,0,1],[2,1,0]]) {
		const ordered = order.map(i => statements[i]);
		await check([ordered.join("\n")], 0);
		await check(ordered, 0);
	}
	await check([table, fk, "CREATE INDEX child_idx ON app.child (b,a);"], 1);
	await check([table, fk], 1);
	await check([table, fk, index], 0);
	await client.sources.remove("sql-index-rules");
}
