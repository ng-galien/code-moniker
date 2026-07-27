#!/usr/bin/env node

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const scriptDir = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(scriptDir, "..");
const require = createRequire(import.meta.url);
const ts = require(path.join(repoRoot, "vscode-extension/node_modules/typescript"));
const tsPackage = require(path.join(
	repoRoot,
	"vscode-extension/node_modules/typescript/package.json",
));
const libDir = path.join(repoRoot, "vscode-extension/node_modules/typescript/lib");
const output =
	process.argv[2] ??
	path.join(repoRoot, "crates/core/src/lang/ts/sdk_catalog_generated.rs");

const libraryFiles = fs
	.readdirSync(libDir)
	.filter((name) => /^lib\..+\.d\.ts$/.test(name) && name !== "lib.d.ts")
	.sort();

const records = new Map();
const dependencies = new Map();
const callableTypeResults = new Map();
const namespaceVariables = [];
const digest = crypto.createHash("sha256");

function libraryName(fileName) {
	return fileName.slice("lib.".length, -".d.ts".length).toLowerCase();
}

function textOf(node, sourceFile) {
	return node?.getText(sourceFile) ?? "";
}

function simpleTypeName(node, sourceFile) {
	if (!node) return "";
	if (ts.isTypeReferenceNode(node)) {
		const name = textOf(node.typeName, sourceFile);
		return name.split(".").at(-1) ?? "";
	}
	if (ts.isExpressionWithTypeArguments(node)) {
		const name = textOf(node.expression, sourceFile);
		return name.split(".").at(-1) ?? "";
	}
	if (ts.isArrayTypeNode(node)) return "Array";
	if (ts.isParenthesizedTypeNode(node)) return simpleTypeName(node.type, sourceFile);
	if (ts.isUnionTypeNode(node) || ts.isIntersectionTypeNode(node)) {
		for (const child of node.types) {
			const name = simpleTypeName(child, sourceFile);
			if (name && name !== "null" && name !== "undefined") return name;
		}
		return "";
	}
	if (ts.isFunctionTypeNode(node)) return simpleTypeName(node.type, sourceFile);
	if (ts.isTypeOperatorNode(node)) return simpleTypeName(node.type, sourceFile);
	if (ts.isIndexedAccessTypeNode(node)) {
		return simpleTypeName(node.objectType, sourceFile);
	}
	switch (node.kind) {
		case ts.SyntaxKind.StringKeyword:
			return "String";
		case ts.SyntaxKind.NumberKeyword:
			return "Number";
		case ts.SyntaxKind.BooleanKeyword:
			return "Boolean";
		case ts.SyntaxKind.ObjectKeyword:
			return "Object";
		default:
			return "";
	}
}

function declarationName(node, sourceFile) {
	if (!node?.name) return "";
	if (ts.isIdentifier(node.name) || ts.isStringLiteral(node.name)) {
		return node.name.text;
	}
	return textOf(node.name, sourceFile);
}

function record(key, data, library) {
	let entry = records.get(key);
	if (!entry) {
		entry = { key, data, libraries: new Set() };
		records.set(key, entry);
	}
	entry.libraries.add(library);
}

function qualifiedName(namespace, name) {
	return namespace.length > 0 ? `${namespace.join(".")}.${name}` : name;
}

function localDeclarationNames(statements, sourceFile) {
	const names = new Set();
	for (const statement of statements) {
		if (
			ts.isInterfaceDeclaration(statement) ||
			ts.isClassDeclaration(statement) ||
			ts.isTypeAliasDeclaration(statement) ||
			ts.isEnumDeclaration(statement) ||
			ts.isModuleDeclaration(statement)
		) {
			const name = declarationName(statement, sourceFile);
			if (name) names.add(name);
		}
	}
	return names;
}

function qualifiedTypeName(node, sourceFile, namespace, localNames) {
	const name = simpleTypeName(node, sourceFile);
	if (!name || name.includes(".") || namespace.length === 0 || !localNames.has(name)) {
		return name;
	}
	return qualifiedName(namespace, name);
}

function visitModule(node, sourceFile, library, parentNamespace) {
	if (!ts.isIdentifier(node.name)) return;
	const name = node.name.text;
	const namespace = [...parentNamespace, name];
	const owner = namespace.join(".");
	record(`T|${owner}`, { kind: "type", name: owner }, library);
	if (parentNamespace.length === 0) {
		record(`V|${owner}|${owner}`, { kind: "value", name: owner, owner }, library);
	} else {
		const parent = parentNamespace.join(".");
		record(
			`M|${parent}|${name}|property|${owner}`,
			{ kind: "member", owner: parent, name, memberKind: "property", result: owner },
			library,
		);
	}
	if (node.body && ts.isModuleBlock(node.body)) {
		visitStatements(node.body.statements, sourceFile, library, namespace);
	} else if (node.body && ts.isModuleDeclaration(node.body)) {
		visitModule(node.body, sourceFile, library, namespace);
	}
}

function visitStatements(statements, sourceFile, library, namespace = []) {
	const localNames = localDeclarationNames(statements, sourceFile);
	for (const statement of statements) {
		if (ts.isModuleDeclaration(statement)) {
			visitModule(statement, sourceFile, library, namespace);
			continue;
		}
		if (
			ts.isInterfaceDeclaration(statement) ||
			ts.isClassDeclaration(statement) ||
			ts.isTypeAliasDeclaration(statement) ||
			ts.isEnumDeclaration(statement)
		) {
			const localName = declarationName(statement, sourceFile);
			if (!localName) continue;
			const owner = qualifiedName(namespace, localName);
			record(`T|${owner}`, { kind: "type", name: owner }, library);

			if (ts.isClassDeclaration(statement) || ts.isEnumDeclaration(statement)) {
				if (namespace.length === 0) {
					record(
						`V|${owner}|${owner}`,
						{ kind: "value", name: owner, owner },
						library,
					);
				} else {
					const namespaceOwner = namespace.join(".");
					record(
						`M|${namespaceOwner}|${localName}|property|${owner}`,
						{
							kind: "member",
							owner: namespaceOwner,
							name: localName,
							memberKind: "property",
							result: owner,
						},
						library,
					);
				}
			}

			for (const heritage of statement.heritageClauses ?? []) {
				for (const type of heritage.types) {
					const parent = qualifiedTypeName(
						type,
						sourceFile,
						namespace,
						localNames,
					);
					if (parent) {
						record(
							`P|${owner}|${parent}`,
							{ kind: "parent", owner, parent },
							library,
						);
					}
				}
			}

			for (const member of statement.members ?? []) {
				if (
					ts.isCallSignatureDeclaration(member) ||
					ts.isConstructSignatureDeclaration(member)
				) {
					const result = qualifiedTypeName(
						member.type,
						sourceFile,
						namespace,
						localNames,
					);
					if (result) callableTypeResults.set(owner, result);
					continue;
				}
				const name = declarationName(member, sourceFile);
				if (!name || name.startsWith("[")) continue;
				const callable =
					ts.isMethodSignature(member) ||
					ts.isMethodDeclaration(member) ||
					(ts.isPropertySignature(member) &&
						member.type &&
						ts.isFunctionTypeNode(member.type));
				const result = qualifiedTypeName(
					member.type,
					sourceFile,
					namespace,
					localNames,
				);
				const memberKind = callable ? "method" : "property";
				record(
					`M|${owner}|${name}|${memberKind}|${result}`,
					{ kind: "member", owner, name, memberKind, result },
					library,
				);
			}
			continue;
		}

		if (ts.isVariableStatement(statement)) {
			for (const declaration of statement.declarationList.declarations) {
				if (!ts.isIdentifier(declaration.name)) continue;
				const name = declaration.name.text;
				const owner = qualifiedTypeName(
					declaration.type,
					sourceFile,
					namespace,
					localNames,
				);
				if (namespace.length === 0) {
					record(
						`V|${name}|${owner}`,
						{ kind: "value", name, owner },
						library,
					);
				} else {
					namespaceVariables.push({
						namespaceOwner: namespace.join("."),
						name,
						typeOwner: owner,
						library,
					});
				}
			}
			continue;
		}

		if (ts.isFunctionDeclaration(statement) && statement.name) {
			const name = statement.name.text;
			if (namespace.length === 0) {
				record(`V|${name}|`, { kind: "value", name, owner: "" }, library);
			} else {
				const owner = namespace.join(".");
				const result = qualifiedTypeName(
					statement.type,
					sourceFile,
					namespace,
					localNames,
				);
				record(
					`M|${owner}|${name}|method|${result}`,
					{ kind: "member", owner, name, memberKind: "method", result },
					library,
				);
			}
		}
	}
}

for (const fileName of libraryFiles) {
	const library = libraryName(fileName);
	const absolute = path.join(libDir, fileName);
	const source = fs.readFileSync(absolute, "utf8");
	digest.update(fileName);
	digest.update("\0");
	digest.update(source);
	const sourceFile = ts.createSourceFile(
		absolute,
		source,
		ts.ScriptTarget.Latest,
		true,
		ts.ScriptKind.TS,
	);
	const refs = ts
		.preProcessFile(source)
		.libReferenceDirectives.map((directive) => directive.fileName.toLowerCase())
		.sort();
	dependencies.set(library, refs);
	visitStatements(sourceFile.statements, sourceFile, library);
}

for (const variable of namespaceVariables) {
	const { namespaceOwner, name, typeOwner, library } = variable;
	record(
		`M|${namespaceOwner}|${name}|property|${typeOwner}`,
		{
			kind: "member",
			owner: namespaceOwner,
			name,
			memberKind: "property",
			result: typeOwner,
		},
		library,
	);
	const result = callableTypeResults.get(typeOwner);
	if (result) {
		record(
			`M|${namespaceOwner}|${name}|method|${result}`,
			{
				kind: "member",
				owner: namespaceOwner,
				name,
				memberKind: "method",
				result,
			},
			library,
		);
	}
}

const entries = [...records.values()].sort((left, right) =>
	compareText(left.key, right.key),
);
entries.forEach((entry, ordinal) => {
	entry.ordinal = ordinal;
});

const types = entries
	.filter((entry) => entry.data.kind === "type")
	.map((entry) => [entry.data.name, entry.ordinal])
	.sort((left, right) => compareText(left[0], right[0]));
const values = entries
	.filter((entry) => entry.data.kind === "value")
	.map((entry) => [entry.data.name, entry.data.owner, entry.ordinal])
	.sort((left, right) =>
		compareText(left[0], right[0]) ||
		compareText(left[1], right[1]) ||
		left[2] - right[2],
	);
const members = entries
	.filter((entry) => entry.data.kind === "member")
	.map((entry) => [
		entry.data.owner,
		entry.data.name,
		entry.data.memberKind === "method",
		entry.data.result,
		entry.ordinal,
	])
	.sort((left, right) =>
		compareText(left[0], right[0]) ||
		compareText(left[1], right[1]) ||
		Number(left[2]) - Number(right[2]) ||
		compareText(left[3], right[3]) ||
		left[4] - right[4],
	);
const parents = entries
	.filter((entry) => entry.data.kind === "parent")
	.map((entry) => [entry.data.owner, entry.data.parent, entry.ordinal])
	.sort((left, right) =>
		compareText(left[0], right[0]) ||
		compareText(left[1], right[1]) ||
		left[2] - right[2],
	);

const libraryOrdinals = new Map(
	libraryFiles.map((fileName) => [libraryName(fileName), []]),
);
for (const entry of entries) {
	for (const library of entry.libraries) {
		libraryOrdinals.get(library)?.push(entry.ordinal);
	}
}

function rust(value) {
	return JSON.stringify(value);
}

function compareText(left, right) {
	return left < right ? -1 : left > right ? 1 : 0;
}

function rustSlice(rows, render) {
	const rendered = [];
	for (const row of rows) {
		rendered.push(`\t${render(row)},`);
	}
	return rendered.join("\n");
}

const libraries = [...libraryOrdinals.entries()]
	.map(([name, ordinals]) => [
		name,
		ordinals.sort((left, right) => left - right),
		dependencies.get(name) ?? [],
	])
	.sort((left, right) => compareText(left[0], right[0]));

const generated = `// @generated by scripts/generate-ts-sdk-catalog.mjs; do not edit.
// Source: TypeScript ${tsPackage.version} standard library declaration files.

pub(super) const CATALOG_TYPESCRIPT_VERSION: &str = ${rust(tsPackage.version)};
pub(super) const CATALOG_DIGEST: &str = ${rust(digest.digest("hex"))};

pub(super) const TYPES: &[(&str, u32)] = &[
${rustSlice(types, ([name, ordinal]) => `(${rust(name)}, ${ordinal})`)}
];

pub(super) const VALUES: &[(&str, &str, u32)] = &[
${rustSlice(values, ([name, owner, ordinal]) => `(${rust(name)}, ${rust(owner)}, ${ordinal})`)}
];

pub(super) const MEMBERS: &[(&str, &str, bool, &str, u32)] = &[
${rustSlice(
	members,
	([owner, name, method, result, ordinal]) =>
		`(${rust(owner)}, ${rust(name)}, ${method}, ${rust(result)}, ${ordinal})`,
)}
];

pub(super) const PARENTS: &[(&str, &str, u32)] = &[
${rustSlice(parents, ([owner, parent, ordinal]) => `(${rust(owner)}, ${rust(parent)}, ${ordinal})`)}
];

pub(super) const LIBRARIES: &[(&str, &[u32], &[&str])] = &[
${rustSlice(
	libraries,
	([name, ordinals, refs]) =>
		`(${rust(name)}, &[${ordinals.join(", ")}], &[${refs.map(rust).join(", ")}])`,
)}
];
`;

fs.writeFileSync(output, generated);
console.log(
	`generated ${path.relative(repoRoot, output)}: ${entries.length} entries, ${libraries.length} libraries`,
);
