import type { SymbolDto } from "../../../daemon/model";
import { CodeBlock } from "../../../webview-lib/CodeBlock";
import { parseCallableName } from "../../../webview-lib/parse";
import { glyphClass, symbolGlyph } from "../../../webview-lib/symbolGlyph";
import type { DetailDocument, DetailPayload } from "../panel";
import { isTypeSymbolKind } from "../usageModel";
import { DetailRow, MetaRow, OpenSourceButton, Section } from "./common";
import { UsagesSection } from "./UsageTree";
import { vscode } from "./vscodeApi";

export function DetailView({ payload }: { payload: DetailPayload }) {
	const symbol = payload.symbol;
	const typeTarget = isTypeSymbolKind(symbol.kind);
	const parsed = parseCallableName(symbol.name);
	return (
		<>
			<div className="header symbol-card">
				<div className="header-top">
					<span className={glyphClass(symbol.kind)}>{symbolGlyph(symbol.kind)}</span>
					<span className="symbol-name">{parsed.base}</span>
					{symbol.visibility === "public" && <span className="pub">pub</span>}
					<span className="header-actions">
						<button
							type="button"
							className="source-link"
							onClick={() => vscode.postMessage({ type: "openExplorer", uri: symbol.uri })}
						>
							Open graph
						</button>
						<OpenSourceButton source={symbol} text="Open source" />
					</span>
				</div>
				{parsed.args.length > 0 && (
					<div className="fncard-args">
						{parsed.args.map((arg, index) => (
							<span key={index} className="argchip">
								<span className="argname">{arg.name}</span>
								{arg.type && <span className="argtype">{arg.type}</span>}
							</span>
						))}
					</div>
				)}
				{symbol.signature && <pre className="signature">{symbol.signature}</pre>}
				<div className="meta">
					<MetaRow label="kind" value={symbol.kind} />
					<MetaRow label="visibility" value={symbol.visibility} />
					<MetaRow label="file" value={symbol.file} />
					{symbol.line_range && (
						<MetaRow label="lines" value={`${symbol.line_range[0]}–${symbol.line_range[1]}`} />
					)}
					<button
						type="button"
						className="moniker-row"
						title="Click to copy the moniker"
						onClick={() => void navigator.clipboard.writeText(symbol.uri)}
					>
						<span className="meta-label">moniker</span>
						<span className="moniker-value">{symbol.uri}</span>
					</button>
				</div>
			</div>
			{payload.members.length > 0 && <MembersSection members={payload.members} />}
			{payload.source && (
				<Section title="Source">
					<CodeBlock source={payload.source} active={symbol.line_range} />
				</Section>
			)}
			<UsagesSection
				title="Incoming usages"
				rows={payload.incoming}
				summary={payload.incomingSummary}
				scope="incoming"
				typeTarget={typeTarget}
			/>
			<UsagesSection
				title="Outgoing usages"
				rows={payload.outgoing}
				summary={payload.outgoingSummary}
				scope="outgoing"
				typeTarget={typeTarget}
			/>
		</>
	);
}

// Direct members of a container symbol; a click walks the detail panel to
// that member, mirroring an IDE's structure pane.
function MembersSection({ members }: { members: SymbolDto[] }) {
	return (
		<Section title={`Members (${members.length})`}>
			<div className="members">
				{members.map((member) => (
					<button
						key={member.uri}
						type="button"
						className="member-row"
						title={member.uri}
						onClick={() => vscode.postMessage({ type: "showSymbol", uri: member.uri })}
					>
						<span className={glyphClass(member.kind)}>{symbolGlyph(member.kind)}</span>
						<span className="member-name">{parseCallableName(member.name).base}</span>
						<span className="member-kind">{member.kind}</span>
						{member.visibility && member.visibility !== "default" && (
							<span className="member-vis">{member.visibility}</span>
						)}
					</button>
				))}
			</div>
		</Section>
	);
}

export function DocumentView({ payload }: { payload: DetailDocument }) {
	return (
		<>
			<div className="header">
				<div className="title">
					<span className="kind">{payload.kind}</span>
					<span className="name">{payload.title}</span>
				</div>
				{payload.description && <div className="description">{payload.description}</div>}
				{payload.meta && payload.meta.length > 0 && (
					<div className="meta">
						{payload.meta.map((row) => (
							<MetaRow key={row.label} label={row.label} value={row.value} />
						))}
					</div>
				)}
			</div>
			{(payload.sections || []).map((section) => (
				<Section key={section.title} title={section.title}>
					{section.text && <pre className="signature">{section.text}</pre>}
					{(section.rows || []).map((row) => (
						<DetailRow key={row.label} label={row.label} value={row.value} />
					))}
				</Section>
			))}
		</>
	);
}
