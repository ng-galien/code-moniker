// Counter widget — exercises TSX surface: typed props, intrinsic tags,
// uppercase components, JSX expression identifiers, event handlers.

import { useState } from "react";
import { Button } from "./button";

// cm: def counter props
export interface CounterProps {
	label: string;
	initial?: number;
}

// cm: def counter component
export function Counter({ label, initial = 0 }: CounterProps) {
	// cm: ref counter calls use state
	const [count, setCount] = useState(initial);
	// cm: ref counter reads button
	return (
		<div className="counter">
			<span>
				{label}: {count}
			</span>
			<Button onClick={() => setCount(count + 1)}>+</Button>
		</div>
	);
}

// cm: def counter list component
export function CounterList({ labels }: { labels: string[] }) {
	// cm: ref counter list maps labels
	// cm: ref counter list reads counter
	return (
		<ul>
			{labels.map((l) => (
				<li key={l}>
					<Counter label={l} />
				</li>
			))}
		</ul>
	);
}
