import { Handle, Position } from "@xyflow/react";

import { MemberRow } from "../../../webview-lib/MemberRow";
import { segmentName } from "../../../shared/identity";
import { postFocus } from "../actions";
import type { ScopeNodeModel } from "./model";

// A scope-level container (package, class, dir, module): it shows what it
// holds — the flattened path a dive would take, then its members — so a
// package of classes is readable without entering every card.
export function ContainerCard({ data }: { data: { node: ScopeNodeModel } }) {
	const node = data.node;
	const { chain, members, hidden } = node.outline;
	const landing = [segmentName(node.id), ...chain].join("/");
	return (
		<div className="containercard" title={`Double-click to dive into ${landing}`}>
			<Handle type="target" position={Position.Top} className="port" />
			<div className="containercard-head">
				<span className="containercard-name">
					{segmentName(node.id)}
					{chain.map((name, index) => (
						<span key={index} className="chain-seg">
							{" / "}
							{name}
						</span>
					))}
				</span>
				<span className="containercard-dive" aria-hidden="true">
					⤵
				</span>
			</div>
			<div className="containercard-meta">
				{node.row.kind} · {node.row.defs} defs
			</div>
			{members.length > 0 && (
				<div className="member-list">
					{members.map((member) => (
						<MemberRow
							key={member.identity}
							kind={member.kind}
							name={member.name}
							title={`Focus ${member.name}`}
							onClick={(event) => {
								event.stopPropagation();
								postFocus(member.identity);
							}}
						/>
					))}
					{hidden > 0 && <div className="member-more">+{hidden} more</div>}
				</div>
			)}
			<div className="fncard-degrees">
				{node.callsIn > 0 && <span className="deg-in">⟵ {node.callsIn}</span>}
				{node.callsOut > 0 && <span className="deg-out">⟶ {node.callsOut}</span>}
			</div>
			<Handle type="source" position={Position.Bottom} className="port" />
		</div>
	);
}
