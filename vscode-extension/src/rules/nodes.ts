import * as vscode from "vscode";

import { ParsedRuleFile, RuleEntry } from "./parse";

export type RuleTreeNode = RuleFolderNode | RuleFileNode | RuleNode | InfoNode;

export interface RuleFolderNode {
	kind: "folder";
	id: string;
	label: string;
	relativePath: string;
	children: RuleTreeNode[];
}

export interface RuleFileNode {
	kind: "file";
	uri: vscode.Uri;
	parsed: ParsedRuleFile;
}

export interface RuleNode {
	kind: "rule";
	uri: vscode.Uri;
	fileFragment?: string;
	rule: RuleEntry;
}

export interface InfoNode {
	kind: "info";
	label: string;
}
