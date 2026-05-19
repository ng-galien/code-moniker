// Plain JSX (no types) — exercises .jsx extension stripping and the same
// intrinsic / uppercase / expression-identifier surface as the .tsx variant.

import { render } from "react-dom";
import { Greeting } from "./greeting";

// cm: def app component
function App() {
	const name = "world";
	// cm: ref app reads greeting
	return (
		<main className="app">
			<Greeting>{name}</Greeting>
			<footer>built</footer>
		</main>
	);
}

// cm: ref render calls react dom
render(<App />, document.getElementById("root"));
