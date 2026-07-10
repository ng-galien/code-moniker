// Bundles the extension host code, notebook renderer and webviews (symbol
// detail, graph explorer).
const esbuild = require("esbuild");
const fs = require("node:fs");
const path = require("node:path");

const watch = process.argv.includes("--watch");
const repoRoot = path.resolve(__dirname, "..");

function samplePackPlugin() {
	return {
		name: "code-moniker-sample-packs",
		setup(build) {
			build.onResolve({ filter: /^code-moniker-sample-packs$/ }, (args) => ({
				path: args.path,
				namespace: "code-moniker-sample-packs",
			}));
			build.onLoad(
				{ filter: /.*/, namespace: "code-moniker-sample-packs" },
				() => ({
					contents: samplePackModule(),
					loader: "ts",
					resolveDir: repoRoot,
				}),
			);
		},
	};
}

function samplePackModule() {
	const groups = [
		{ category: "sample", dir: "samples/catalog" },
		{ category: "learn", dir: "samples/learn" },
	];
	const imports = [];
	const entries = [];
	let index = 0;
	for (const group of groups) {
		const dir = path.join(repoRoot, group.dir);
		const names = fs
			.readdirSync(dir)
			.filter((name) => name.endsWith(".cm.md"))
			.sort();
		for (const name of names) {
			const variable = `document${index++}`;
			imports.push(
				`import ${variable} from ${JSON.stringify(path.join(dir, name))};`,
			);
			entries.push(
				`{ category: ${JSON.stringify(group.category)} as const, document: ${variable} }`,
			);
		}
	}
	return `${imports.join("\n")}

export const CATALOG_DOCUMENTS = [
	${entries.join(",\n\t")}
];
`;
}

const extensionConfig = {
	entryPoints: ["src/extension.ts"],
	bundle: true,
	outfile: "dist/extension.js",
	platform: "node",
	format: "cjs",
	target: "node18",
	external: ["vscode", "bufferutil", "utf-8-validate"],
	loader: { ".md": "text" },
	plugins: [samplePackPlugin()],
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

const detailWebviewConfig = {
	entryPoints: ["src/symbols/detail/webview.ts"],
	bundle: true,
	outfile: "media/symbols/detail.js",
	platform: "browser",
	format: "iife",
	target: "es2022",
};

// React webview: JSX via the automatic runtime, NODE_ENV pinned so react-dom
// bundles its production build, minified because the artifact is committed.
// The CSS imported from the entry point lands in media/explorer/explorer.css.
const explorerWebviewConfig = {
	entryPoints: ["src/explorer/webview/index.tsx"],
	bundle: true,
	outfile: "media/explorer/explorer.js",
	platform: "browser",
	format: "iife",
	target: "es2022",
	jsx: "automatic",
	define: { "process.env.NODE_ENV": '"production"' },
	minify: true,
};

async function main() {
	if (watch) {
		const a = await esbuild.context(extensionConfig);
		const b = await esbuild.context(rendererConfig);
		const c = await esbuild.context(detailWebviewConfig);
		const d = await esbuild.context(explorerWebviewConfig);
		await Promise.all([a.watch(), b.watch(), c.watch(), d.watch()]);
		console.log("esbuild watching…");
	} else {
		await Promise.all([
			esbuild.build(extensionConfig),
			esbuild.build(rendererConfig),
			esbuild.build(detailWebviewConfig),
			esbuild.build(explorerWebviewConfig),
		]);
		console.log("esbuild build complete");
	}
}

main().catch((err) => {
	console.error(err);
	process.exit(1);
});
