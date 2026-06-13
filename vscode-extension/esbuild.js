// Bundles the extension host code (CommonJS) and the scenario output renderer (ESM).
const esbuild = require("esbuild");

const watch = process.argv.includes("--watch");

const extensionConfig = {
	entryPoints: ["src/extension.ts"],
	bundle: true,
	outfile: "dist/extension.js",
	platform: "node",
	format: "cjs",
	target: "node18",
	external: ["vscode"],
	loader: { ".md": "text" },
	sourcemap: true,
};

const rendererConfig = {
	entryPoints: ["renderer/violations.ts"],
	bundle: true,
	outfile: "dist/renderer.js",
	platform: "browser",
	format: "esm",
	target: "es2022",
	sourcemap: true,
};

async function main() {
	if (watch) {
		const a = await esbuild.context(extensionConfig);
		const b = await esbuild.context(rendererConfig);
		await Promise.all([a.watch(), b.watch()]);
		console.log("esbuild watching…");
	} else {
		await Promise.all([
			esbuild.build(extensionConfig),
			esbuild.build(rendererConfig),
		]);
		console.log("esbuild build complete");
	}
}

main().catch((err) => {
	console.error(err);
	process.exit(1);
});
