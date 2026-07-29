// Public wire-model surface for the extension. The reusable client owns the
// schema-generated DTOs and protocol version; this façade keeps existing
// feature imports stable while the extension consumes that single source.
export * from "@code-moniker/client";

export type LineRange = [number, number];
