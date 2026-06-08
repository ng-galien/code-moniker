import * as vscode from "vscode";

import { ParsedRuleFile, RuleEntry } from "./parse";

export type RuleTreeNode = RuleFileNode | RuleNode | InfoNode;

export interface RuleFileNode {
	kind: "file";
	uri: vscode.Uri;
	parsed: ParsedRuleFile;
}

export interface RuleNode {
	kind: "rule";
	uri: vscode.Uri;
	rule: RuleEntry;
}

export interface InfoNode {
	kind: "info";
	label: string;
}
