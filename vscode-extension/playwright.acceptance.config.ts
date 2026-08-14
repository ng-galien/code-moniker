import { defineConfig } from "@playwright/test";

export default defineConfig({
	testDir: "./acceptance/specs",
	testMatch: ["**/*.spec.ts"],
	globalSetup: "./acceptance/globalSetup.ts",
	fullyParallel: false,
	workers: 1,
	maxFailures: 1,
	preserveOutput: "always",
	timeout: 60_000,
	expect: { timeout: 5_000 },
	forbidOnly: Boolean(process.env.CI),
	retries: 0,
	reporter: process.env.CI
		? [["list"], ["junit", { outputFile: "test-results/acceptance-junit.xml" }]]
		: "list",
	use: {
		trace: "retain-on-failure",
		screenshot: "only-on-failure",
		video: "retain-on-failure",
	},
	outputDir: "test-results/acceptance",
});
