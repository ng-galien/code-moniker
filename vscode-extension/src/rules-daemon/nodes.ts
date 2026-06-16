import { RuleDto, ViolationDto } from "../daemon/model";

export type RulesTreeNode = SectionNode | RuleNode | GroupNode | ViolationNode | InfoNode;

export interface SectionNode {
	kind: "section";
	id: "rules" | "check";
	label: string;
}

export interface RuleNode {
	kind: "rule";
	rule: RuleDto;
}

export interface GroupNode {
	kind: "group";
	root: string;
	file: string;
	violations: ViolationDto[];
}

export interface ViolationNode {
	kind: "violation";
	violation: ViolationDto;
}

export interface InfoNode {
	kind: "info";
	label: string;
}
