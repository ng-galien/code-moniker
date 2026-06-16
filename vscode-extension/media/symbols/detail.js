// Reactive renderer for the symbol detail webview. Receives `{ type: "detail",
// payload }` messages and rebuilds the panel from scratch. All text goes through
// textContent so symbol/source content is never interpreted as HTML.
(function () {
	const vscode = acquireVsCodeApi();
	const root = document.getElementById("root");

	window.addEventListener("message", (event) => {
		const message = event.data;
		if (message && message.type === "detail") {
			render(message.payload);
		}
	});

	function render(payload) {
		root.className = "";
		root.replaceChildren();
		root.appendChild(header(payload.symbol));
		if (payload.source) {
			root.appendChild(sourceSection(payload.symbol, payload.source));
		}
		root.appendChild(
			usagesSection("Incoming usages", payload.incoming, payload.incomingSummary),
		);
		root.appendChild(
			usagesSection("Outgoing usages", payload.outgoing, payload.outgoingSummary),
		);
	}

	function header(symbol) {
		const box = el("div", "header");
		const title = el("div", "title");
		title.appendChild(el("span", "kind", symbol.kind));
		title.appendChild(el("span", "name", symbol.name));
		box.appendChild(title);

		if (symbol.signature) {
			box.appendChild(el("pre", "signature", symbol.signature));
		}

		const meta = el("div", "meta");
		meta.appendChild(metaRow("visibility", symbol.visibility));
		meta.appendChild(metaRow("file", symbol.file));
		if (symbol.line_range) {
			meta.appendChild(metaRow("lines", symbol.line_range[0] + "–" + symbol.line_range[1]));
		}
		meta.appendChild(metaRow("moniker", symbol.uri));
		box.appendChild(meta);

		const open = el("button", "open", "Open source");
		open.addEventListener("click", () => {
			vscode.postMessage({
				type: "openSource",
				target: {
					root: symbol.root,
					file: symbol.file,
					line: symbol.line_range ? symbol.line_range[0] : 1,
				},
			});
		});
		box.appendChild(open);
		return box;
	}

	function sourceSection(symbol, source) {
		const box = section("Source");
		const code = el("div", "code");
		const active = symbol.line_range;
		for (const line of source.lines) {
			const row = el("div", "code-line");
			if (active && line.number >= active[0] && line.number <= active[1]) {
				row.classList.add("active");
			}
			row.appendChild(el("span", "gutter", String(line.number)));
			row.appendChild(el("span", "src", line.text || " "));
			code.appendChild(row);
		}
		box.body.appendChild(code);
		return box.root;
	}

	function usagesSection(title, rows, summary) {
		const box = section(title + " (" + rows.length + ")");
		if (summary && summary.dominant_prefix) {
			box.body.appendChild(el("div", "summary", summary.shared_helper_signal + " · " + summary.dominant_prefix));
		}
		if (rows.length === 0) {
			box.body.appendChild(el("div", "empty-row", "none"));
			return box.root;
		}
		for (const usage of rows) {
			const row = el("div", "usage");
			row.appendChild(el("span", "usage-kind", usage.kind));
			row.appendChild(el("span", "usage-actor", usage.actor || usage.context || usage.endpoint));
			row.appendChild(el("span", "usage-loc", usage.file + ":" + usage.location));
			row.addEventListener("click", () => {
				vscode.postMessage({
					type: "openSource",
					target: {
						root: usage.root,
						file: usage.file,
						line: usage.line_range ? usage.line_range[0] : 1,
					},
				});
			});
			box.body.appendChild(row);
		}
		return box.root;
	}

	function section(titleText) {
		const root = el("div", "section");
		root.appendChild(el("div", "section-title", titleText));
		const body = el("div", "section-body");
		root.appendChild(body);
		return { root, body };
	}

	function metaRow(label, value) {
		const row = el("div", "meta-row");
		row.appendChild(el("span", "meta-label", label));
		row.appendChild(el("span", "meta-value", value || "—"));
		return row;
	}

	function el(tag, className, text) {
		const node = document.createElement(tag);
		if (className) {
			node.className = className;
		}
		if (text !== undefined) {
			node.textContent = text;
		}
		return node;
	}
})();
