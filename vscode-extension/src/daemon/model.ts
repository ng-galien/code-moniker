// Public wire-model surface for the extension. The DTOs are generated from the
// daemon's JSON Schema (`npm run generate:daemon-types`) so they can never drift
// from `crates/query`. This façade re-exports them and adds the few things the
// schema cannot carry: the line-range tuple alias and the protocol version.
export * from "./generated";

export type LineRange = [number, number];

export const PROTOCOL_VERSION = 1;
