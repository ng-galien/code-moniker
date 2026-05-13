# `code-moniker manifest`

Extract declared dependencies from a build manifest and surface them as
records that bind, by moniker, to the refs the per-language extractor
already emits for cross-package imports.

```
code-moniker manifest <PATH> [--format tsv|json|tree]
                             [--count] [--quiet]
                             [--scheme <SCHEME>]
```

| `<PATH>` | Output |
| -------- | ------ |
| file      | rows from a single manifest (auto-detected by filename) |
| directory | rows from every recognised manifest under the tree (gitignore-aware) |

## Filename auto-detection

| Pattern         | Manifest      | Lang |
| --------------- | ------------- | ---- |
| `Cargo.toml`    | `cargo`       | rs   |
| `package.json`  | `package_json`| ts   |
| `pom.xml`       | `pom_xml`     | java |
| `pyproject.toml`| `pyproject`   | python |
| `go.mod`        | `go_mod`      | go   |
| `*.csproj`      | `csproj`      | cs   |

A file whose name does not match exits `2`. A directory containing no
recognised manifest exits `1` with empty output.

## Output columns

| Column            | Description |
| ----------------- | ----------- |
| `package_moniker` | Moniker URI under which the extractor emits `external_pkg:` refs for this dep. Bind key. |
| `manifest_uri`    | Path of the manifest file the row was read from (relative when scanning a directory). |
| `name`            | Coordinate as declared in the manifest (Maven `groupId:artifactId`, npm name, crate name, …). |
| `import_root`     | Identifier the source code writes — Cargo `serde-json` → `serde_json`, Python `requests-html` → `requests_html`. |
| `version`         | Version spec string, or empty when the manifest declares a path/workspace-only entry. |
| `dep_kind`        | `package` (the manifest's own self-declared entry) or one of `normal`, `dev`, `peer`, `optional`, `build`, `compile`, `test`, `indirect`, `project`, `optional:<group>`, depending on manifest. |

The moniker is constructed via each language's `external_pkg` rule so it
is byte-identical to the head every extractor uses for the same import:

| Manifest      | `import_root` shape | `package_moniker` segments |
| ------------- | ------------------- | -------------------------- |
| `cargo`       | single identifier   | `external_pkg:<root>` |
| `package_json`| simple or `@scope/pkg` | `external_pkg:<root>` (scope preserved) |
| `pyproject`   | single identifier (normalised) | `external_pkg:<root>` |
| `go_mod`      | slash-separated module path | `external_pkg:<head>/path:<piece>/…` |
| `csproj`      | dot-separated namespace | `external_pkg:<head>/path:<piece>/…` |
| `pom_xml`     | `groupId:artifactId` | `external_pkg:<coord>` (does not currently bind; see *Limitations*) |

## Output formats

### TSV (default)

```
<package_moniker><TAB><manifest_uri><TAB><name><TAB><import_root><TAB><version><TAB><dep_kind>
```

Empty fields render literally as empty strings (e.g., path-only Cargo
dep keeps an empty version column).

### JSON

```json
[
  {
    "package_moniker": "code+moniker://./external_pkg:react",
    "manifest_uri": "package.json",
    "manifest_kind": "package_json",
    "name": "react",
    "import_root": "react",
    "version": "^18.0.0",
    "dep_kind": "normal"
  }
]
```

### Tree

```
package.json
  ├─ react        ^18.0.0  (normal)  code+moniker://./external_pkg:react
  └─ vitest       1.0.0    (dev)     code+moniker://./external_pkg:vitest
sub/Cargo.toml
  ├─ demo         0.1.0    (package) code+moniker://./external_pkg:demo
  └─ serde_json   1        (normal)  code+moniker://./external_pkg:serde_json
```

## Binding contract

For TS, RS, Python (stdlib only), Go, and C#, the helper produces a
moniker byte-identical to the head every extractor emits for the same
import. Consumers join on:

```sql
SELECT *
FROM linkage
JOIN repo_dep ON repo_dep.package_moniker @> linkage.target_moniker;
```

`code-moniker-core` ships a test that exercises this for each supported
lang; see `crates/core/src/lang/build_manifest.rs::tests::package_moniker_binds_extractor_ref_per_language`.

## Limitations

- **Java**. The extractor uses `lang:java/package:…` for non-stdlib
  imports, so the Maven coordinate emitted in `package_moniker` will not
  `@>`-bind to any extractor ref target. The column is still emitted
  for shape uniformity.
- **Python non-stdlib**. Same issue — the extractor routes non-stdlib
  imports through `lang:python/package:…`. The manifest's
  `package_moniker` is still useful as a stable identifier but does not
  bind via `@>`.
- **Stdlib roots**. JDK / node / Go stdlib are not declared in any
  manifest; consumers seed those rows out-of-band.

## Exit codes

| Code | Meaning |
| ---- | ------- |
| 0    | at least one row was emitted |
| 1    | no rows (empty directory / manifest with zero deps) |
| 2    | usage error (unknown filename, malformed manifest) |
