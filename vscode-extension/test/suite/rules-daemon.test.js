const assert = require("node:assert");
const vscode = require("vscode");

const { collectSymbols, getApi, waitFor, waitForReady } = require("./helpers");

// Feature 3: the rules section lists the active rules, running a check produces
// grouped violations, and those violations decorate the symbol tree.
async function testRulesDaemon() {
	const api = await getApi();
	await waitForReady(api);
	api.rules.refresh();

	const sections = await api.rules.getChildren();
	const rulesSection = sections.find((node) => node.kind === "section" && node.id === "rules");
	const checkSection = sections.find((node) => node.kind === "section" && node.id === "check");
	assert.ok(rulesSection && checkSection, "rules and check sections should exist");

	const ruleNodes = await waitFor(async () => {
		const children = await api.rules.getChildren(rulesSection);
		const rules = children.filter((node) => node.kind === "rule");
		return rules.length > 0 ? rules : undefined;
	}, "the active rule set to load");
	assert.ok(
		ruleNodes.some((node) => node.rule.id.includes("function-snake-case")),
		`seeded rule should be listed, got ${ruleNodes.map((n) => n.rule.id).join(", ")}`,
	);

	await vscode.commands.executeCommand("codeMoniker.rulesDaemon.runCheck");

	const groups = await waitFor(async () => {
		const children = await api.rules.getChildren(checkSection);
		const found = children.filter((node) => node.kind === "group");
		return found.length > 0 ? found : undefined;
	}, "check to produce violation groups");

	const libGroup = groups.find((node) => node.file === "src/lib.rs");
	assert.ok(libGroup, "violations should be grouped under src/lib.rs");
	const violations = await api.rules.getChildren(libGroup);
	assert.ok(
		violations.some(
			(node) => node.kind === "violation" && node.violation.rule_id.includes("function-snake-case"),
		),
		"the snake-case rule should report a violation",
	);

	// The violation overlays the symbol tree and the affected symbol.
	assert.ok(api.violations.fileViolations("src/lib.rs") >= 1, "file should carry a violation count");

	const symbols = await collectSymbols(api.symbols, {
		kind: "entry",
		tree: { root: api.session.workspaceRoots[0], path: "src/lib.rs", kind: "file", language: "rust", defs: 0, refs: 0, change_count: 0 },
	});
	const doThing = symbols.find((node) => node.symbol.name.startsWith("DoThing"));
	assert.ok(doThing, "DoThing symbol should be present");
	assert.ok(
		api.violations.symbolViolations(doThing.symbol) >= 1,
		"DoThing should carry the violation overlay",
	);

	console.log("rules in tree + decorations: ok");
}

module.exports = { testRulesDaemon };
