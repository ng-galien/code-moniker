import { CodeBlock } from "../../../webview-lib/CodeBlock";
import type { DetailDocument, DetailPayload } from "../panel";
import { DetailRow, MetaRow, OpenSourceButton, Section } from "./common";
import { UsagesSection } from "./UsageTree";

export function DetailView({ payload }: { payload: DetailPayload }) {
	const symbol = payload.symbol;
	return (
		<>
			<div className="header">
				<div className="header-top">
					<div className="title">
						<span className="kind">{symbol.kind}</span>
						<span className="name">{symbol.name}</span>
					</div>
					<OpenSourceButton source={symbol} text="Open source" />
				</div>
				{symbol.signature && <pre className="signature">{symbol.signature}</pre>}
				<div className="meta">
					<MetaRow label="visibility" value={symbol.visibility} />
					<MetaRow label="file" value={symbol.file} />
					{symbol.line_range && (
						<MetaRow label="lines" value={`${symbol.line_range[0]}–${symbol.line_range[1]}`} />
					)}
					<MetaRow label="moniker" value={symbol.uri} />
				</div>
			</div>
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
			/>
			<UsagesSection
				title="Outgoing usages"
				rows={payload.outgoing}
				summary={payload.outgoingSummary}
				scope="outgoing"
			/>
		</>
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
