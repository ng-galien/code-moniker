// Graph explorer webview: renders the ego-centric triptych from posted
// "unit" messages. Facts only; clicks post focus/openSource messages back.
(function () {
	const vscode = acquireVsCodeApi();
	const root = document.getElementById("root");

	window.addEventListener("message", (event) => {
		const message = event.data;
		if (message?.type === "unit") {
			render(message.payload);
		}
	});

	vscode.postMessage({ type: "ready" });

	function render(unit) {
		root.classList.remove("empty");
		root.innerHTML = "";
		root.appendChild(toolbar(unit));
		const grid = el("div", "triptych");
		grid.appendChild(neighborColumn("Callers", unit.callers, unit, "left"));
		grid.appendChild(centerColumn(unit));
		grid.appendChild(neighborColumn("Callees", unit.callees, unit, "right"));
		root.appendChild(grid);
	}

	function toolbar(unit) {
		const bar = el("div", "toolbar");
		const back = navButton("←", unit.canBack, () => vscode.postMessage({ type: "back" }));
		const forward = navButton("→", unit.canForward, () => vscode.postMessage({ type: "forward" }));
		bar.appendChild(back);
		bar.appendChild(forward);
		if (unit.focus.kind === "symbol") {
			const crumb = el("span", "crumb link");
			crumb.textContent = unit.focus.symbol.file;
			crumb.addEventListener("click", () => {
				vscode.postMessage({ type: "focus", uri: unit.focus.symbol.file });
			});
			bar.appendChild(crumb);
			const sep = el("span", "crumb-sep");
			sep.textContent = "▸";
			bar.appendChild(sep);
		}
		const label = el("span", "focus-label");
		label.textContent = unit.focus.kind === "symbol"
			? `${unit.focus.symbol.kind} ${unit.focus.symbol.name}`
			: `file ${unit.focus.path}`;
		bar.appendChild(label);
		if (unit.unresolvedRefs > 0) {
			const unresolved = el("span", "unresolved");
			unresolved.textContent = `${unit.unresolvedRefs} unresolved ref(s)`;
			bar.appendChild(unresolved);
		}
		return bar;
	}

	function navButton(text, enabled, onClick) {
		const button = el("button", "nav");
		button.textContent = text;
		button.disabled = !enabled;
		button.addEventListener("click", onClick);
		return button;
	}

	function neighborColumn(title, neighbors, unit, side) {
		const column = el("div", `column ${side}`);
		column.appendChild(heading(`${title} (${neighbors.length})`));
		if (neighbors.length === 0) {
			column.appendChild(muted(side === "left" ? "no callers" : "no external callees"));
			return column;
		}
		const focusUri = unit.focus.kind === "symbol" ? unit.focus.symbol.uri : null;
		for (const neighbor of neighbors) {
			column.appendChild(neighborRow(neighbor, focusUri));
		}
		return column;
	}

	function neighborRow(neighbor, focusUri) {
		const row = el("div", "neighbor");
		const recursion = focusUri && neighbor.symbol.uri === focusUri;
		const name = el("div", "name");
		name.textContent = `${recursion ? "↺ " : ""}${neighbor.symbol.kind} ${neighbor.symbol.name}`;
		row.appendChild(name);
		const meta = el("div", "meta");
		const count = neighbor.count > 1 ? ` ×${neighbor.count}` : "";
		meta.textContent = `${neighbor.symbol.file}${count} [${neighbor.kinds.join(",")}]`;
		row.appendChild(meta);
		row.addEventListener("click", () => {
			vscode.postMessage({ type: "focus", uri: neighbor.symbol.uri });
		});
		row.addEventListener("contextmenu", (event) => {
			event.preventDefault();
			openNeighbor(neighbor.symbol);
		});
		return row;
	}

	function openNeighbor(symbol) {
		vscode.postMessage({
			type: "openSource",
			target: {
				root: symbol.root,
				file: symbol.file,
				line: symbol.line_range ? symbol.line_range[0] : 1,
			},
		});
	}

	function centerColumn(unit) {
		const column = el("div", "column center");
		if (unit.focus.kind === "symbol") {
			column.appendChild(symbolHeader(unit.focus.symbol));
			if (unit.source) {
				column.appendChild(codeBlock(unit.source));
			}
			return column;
		}
		column.appendChild(heading(`members (${unit.members.length})`));
		const counts = internalCounts(unit.internalEdges);
		const focusUri = unit.focus.kind === "symbol" ? unit.focus.symbol.uri : null;
		for (const node of nestMembers(unit.members, focusUri)) {
			column.appendChild(surfaceBox(node, counts));
		}
		return column;
	}

	// Line-range containment nesting: a member's parent is the tightest
	// enclosing member. Mirrors the symbol tree's outline reconstruction.
	function nestMembers(members, focusUri) {
		const ranged = members
			.filter((member) => member.line_range != null && member.uri !== focusUri)
			.slice()
			.sort((a, b) => a.line_range[0] - b.line_range[0] || b.line_range[1] - a.line_range[1]);
		const roots = [];
		const stack = [];
		for (const member of ranged) {
			const node = { member, children: [] };
			while (stack.length > 0 && !contains(stack[stack.length - 1].member, member)) {
				stack.pop();
			}
			if (stack.length === 0) {
				roots.push(node);
			} else {
				stack[stack.length - 1].children.push(node);
			}
			stack.push(node);
		}
		for (const member of members) {
			if (member.line_range == null && member.uri !== focusUri) {
				roots.push({ member, children: [] });
			}
		}
		return roots;
	}

	function contains(outer, inner) {
		const [os, oe] = outer.line_range;
		const [is, ie] = inner.line_range;
		return os <= is && oe >= ie && (os < is || oe > ie);
	}

	function surfaceBox(node, counts) {
		const box = el("div", "surface");
		const row = el("div", "surface-row");
		const name = el("span", "name");
		name.textContent = `${node.member.kind} ${node.member.name}`;
		row.appendChild(name);
		const meta = el("span", "meta");
		const line = node.member.line_range ? `L${node.member.line_range[0]}` : "";
		const internal = counts.get(node.member.id) ?? 0;
		meta.textContent = internal > 0 ? `${line} · ${internal} edge(s)` : line;
		row.appendChild(meta);
		row.addEventListener("click", (event) => {
			event.stopPropagation();
			vscode.postMessage({ type: "focus", uri: node.member.uri });
		});
		box.appendChild(row);
		for (const child of node.children) {
			box.appendChild(surfaceBox(child, counts));
		}
		return box;
	}

	function symbolHeader(symbol) {
		const header = el("div", "unit-header");
		const title = el("div", "title");
		title.textContent = `${symbol.kind} ${symbol.name}`;
		header.appendChild(title);
		if (symbol.signature) {
			const signature = el("div", "signature");
			signature.textContent = symbol.signature;
			header.appendChild(signature);
		}
		const location = el("div", "meta");
		location.textContent = symbol.line_range
			? `${symbol.file} · L${symbol.line_range[0]}-${symbol.line_range[1]}`
			: symbol.file;
		location.classList.add("link");
		location.addEventListener("click", () => openNeighbor(symbol));
		header.appendChild(location);
		return header;
	}

	function internalCounts(edges) {
		const counts = new Map();
		for (const edge of edges) {
			counts.set(edge.source, (counts.get(edge.source) ?? 0) + edge.count);
			counts.set(edge.target, (counts.get(edge.target) ?? 0) + edge.count);
		}
		return counts;
	}

	function codeBlock(source) {
		const pre = el("pre", "code");
		for (const line of source.lines) {
			const row = el("div", "code-line");
			const number = el("span", "line-number");
			number.textContent = String(line.number);
			row.appendChild(number);
			const content = el("span", "line-content");
			for (const token of line.tokens) {
				const span = document.createElement("span");
				span.textContent = token.text;
				if (token.darkColor) {
					span.style.setProperty("--dark", token.darkColor);
				}
				if (token.lightColor) {
					span.style.setProperty("--light", token.lightColor);
				}
				span.className = "tok";
				content.appendChild(span);
			}
			row.appendChild(content);
			pre.appendChild(row);
		}
		return pre;
	}

	function heading(text) {
		const node = el("div", "heading");
		node.textContent = text;
		return node;
	}

	function muted(text) {
		const node = el("div", "muted");
		node.textContent = text;
		return node;
	}

	function el(tag, className) {
		const node = document.createElement(tag);
		node.className = className;
		return node;
	}
})();
