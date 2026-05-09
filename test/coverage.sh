#!/usr/bin/env bash
# Run cargo unit tests + the pgTAP suite under llvm-cov instrumentation
# and produce a unified coverage report.
#
# Why both: cargo test --lib exercises core/ + lang/ but not pg/ (the
# pgrx wrappers). pgTAP exercises the SQL surface end-to-end through
# a running postmaster. Sharing the instrumented build between the two
# yields a single report covering every code path the extension takes
# in production.
#
# Workflow:
#   1. Wipe stale .profraw files and the cached pg_code_moniker build
#      artefacts. cargo's fingerprint does not reliably invalidate on
#      RUSTFLAGS toggles, so a previous non-instrumented build would
#      silently shadow the instrumented rebuild.
#   2. Set RUSTFLAGS to merge -Cinstrument-coverage with the
#      .cargo/config.toml link flags. cargo replaces config rustflags
#      with the env value rather than appending, so the macOS
#      dynamic_lookup flags are re-emitted here.
#   3. Build the instrumented test binary (cargo test --no-run) and
#      install the instrumented extension. Restart pgrx so backends
#      inherit LLVM_PROFILE_FILE.
#   4. Run cargo test (writes profraws via the test binary) and the
#      pgTAP suite (writes profraws via Postgres backends loading the
#      dylib).
#   5. Stop the postmaster so backends flush their counters.
#   6. Merge profraws with llvm-profdata, then llvm-cov report against
#      both binaries.
#
# Requires: llvm-tools-preview (provides llvm-profdata and llvm-cov),
# cargo-pgrx.

set -euo pipefail

PG_VERSION="${PG_VERSION:-pg17}"
PG_CONFIG="${PG_CONFIG:-$HOME/.pgrx/17.9/pgrx-install/bin/pg_config}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cd "$REPO_ROOT"

LLVM_BIN="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | awk '/^host/ {print $2}')/bin"
LLVM_PROFDATA="$LLVM_BIN/llvm-profdata"
LLVM_COV="$LLVM_BIN/llvm-cov"
for bin in "$LLVM_PROFDATA" "$LLVM_COV"; do
	[ -x "$bin" ] || { echo "missing $bin (rustup component add llvm-tools-preview)" >&2; exit 1; }
done

# --- 1. Wipe stale state.
COV_DIR="$REPO_ROOT/target/cov"
rm -rf "$COV_DIR"
mkdir -p "$COV_DIR"
cargo clean -p pg_code_moniker >/dev/null 2>&1 || true

# --- 2. Coverage env. -Cinstrument-coverage merged with the macOS
# dynamic_lookup link flags from .cargo/config.toml.
case "$(uname)" in
	Darwin)
		export RUSTFLAGS="-Clink-arg=-undefined -Clink-arg=dynamic_lookup -Cinstrument-coverage"
		;;
	*)
		export RUSTFLAGS="-Cinstrument-coverage"
		;;
esac

export LLVM_PROFILE_FILE="$COV_DIR/profraw-%p-%10m.profraw"

# --- 3. Build instrumented test binary, capture its path, then install
# the extension.
TEST_BIN="$(cargo test --features pg17 --no-default-features --lib --no-run --message-format=json 2>/dev/null \
	| awk -F\" '/"profile":\{[^}]*"test":true\}/ && /"executable":/ {
		for (i = 1; i <= NF; i++) if ($i == "executable") { print $(i+2); exit }
	}')"
if [ -z "$TEST_BIN" ] || [ ! -x "$TEST_BIN" ]; then
	echo "coverage.sh: could not locate cargo test binary" >&2
	exit 1
fi

cargo pgrx stop "$PG_VERSION" 2>/dev/null || true
cargo pgrx install --pg-config "$PG_CONFIG"

INSTALLED_DYLIB="$(dirname "$PG_CONFIG")/../lib/postgresql/pg_code_moniker.dylib"
[ -f "$INSTALLED_DYLIB" ] || INSTALLED_DYLIB="$(dirname "$PG_CONFIG")/../lib/postgresql/pg_code_moniker.so"
# Sanity check: instrumentation symbols present in the installed
# .dylib. Without them every backend would produce a zero-byte
# .profraw and pg/* coverage would silently read 0%.
nm_count="$(nm "$INSTALLED_DYLIB" 2>&1 | grep -c '__llvm_profile' || true)"
if [ "$nm_count" -eq 0 ]; then
	echo "coverage.sh: installed extension at $INSTALLED_DYLIB lacks __llvm_profile symbols" >&2
	echo "coverage.sh: cargo pgrx install did not honour RUSTFLAGS=$RUSTFLAGS" >&2
	exit 2
fi

cargo pgrx start "$PG_VERSION"
trap 'cargo pgrx stop "$PG_VERSION" 2>/dev/null || true' EXIT

# --- 4. Run the test binary and the pgTAP suite. The test binary uses
# rustc's harness (--test) so we forward any args the user passed.
"$TEST_BIN" --test-threads=1 >/dev/null

pgtap_status=0
./test/run.sh || pgtap_status=$?

# --- 5. Stop pg so each backend's counters are flushed to disk.
cargo pgrx stop "$PG_VERSION"
trap - EXIT

# --- 6. Merge and report.
PROFRAWS=("$COV_DIR"/*.profraw)
if [ ! -e "${PROFRAWS[0]}" ]; then
	echo "coverage.sh: no .profraw files were produced" >&2
	exit 3
fi

"$LLVM_PROFDATA" merge -sparse "${PROFRAWS[@]}" -o "$COV_DIR/merged.profdata"

"$LLVM_COV" report \
	--instr-profile="$COV_DIR/merged.profdata" \
	--object="$TEST_BIN" \
	--object="$INSTALLED_DYLIB" \
	--ignore-filename-regex='/.cargo/registry/|/vendor/|/rustc/|/rustlib/|/.rustup/|/target/debug/build/'

if [ "$pgtap_status" -ne 0 ]; then
	echo "# coverage.sh: pgTAP exited $pgtap_status (report generated above)" >&2
fi

exit "$pgtap_status"
