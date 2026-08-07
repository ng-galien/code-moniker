import { copyFileSync } from "node:fs";

copyFileSync(
	new URL("../../THIRD_PARTY_NOTICES", import.meta.url),
	new URL("../THIRD_PARTY_NOTICES", import.meta.url),
);
