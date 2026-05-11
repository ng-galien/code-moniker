#!/usr/bin/env bash
# Run cargo --lib tests + the pgTAP suite under -Cinstrument-coverage
# and emit a unified llvm-cov report covering core/, lang/, and pg/.

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

COV_DIR="$REPO_ROOT/target/cov"
rm -rf "$COV_DIR"
mkdir -p "$COV_DIR"
cargo clean -p code-moniker >/dev/null 2>&1 || true

case "$(uname)" in
	Darwin)
		export RUSTFLAGS="-Clink-arg=-undefined -Clink-arg=dynamic_lookup -Cinstrument-coverage"
		;;
	*)
		export RUSTFLAGS="-Cinstrument-coverage"
		;;
esac

export LLVM_PROFILE_FILE="$COV_DIR/profraw-%p-%10m.profraw"

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

INSTALLED_DYLIB="$(dirname "$PG_CONFIG")/../lib/postgresql/code_moniker.dylib"
[ -f "$INSTALLED_DYLIB" ] || INSTALLED_DYLIB="$(dirname "$PG_CONFIG")/../lib/postgresql/code_moniker.so"
nm_count="$(nm "$INSTALLED_DYLIB" 2>&1 | grep -c '__llvm_profile' || true)"
if [ "$nm_count" -eq 0 ]; then
	echo "coverage.sh: installed $INSTALLED_DYLIB lacks __llvm_profile symbols" >&2
	exit 2
fi

cargo pgrx start "$PG_VERSION"
trap 'cargo pgrx stop "$PG_VERSION" 2>/dev/null || true' EXIT

"$TEST_BIN" --test-threads=1 >/dev/null

pgtap_status=0
./test/run.sh || pgtap_status=$?

cargo pgrx stop "$PG_VERSION"
trap - EXIT

PROFRAWS=("$COV_DIR"/*.profraw)
if [ ! -e "${PROFRAWS[0]}" ]; then
	echo "coverage.sh: no .profraw files produced" >&2
	exit 3
fi

"$LLVM_PROFDATA" merge -sparse "${PROFRAWS[@]}" -o "$COV_DIR/merged.profdata"

"$LLVM_COV" report \
	--instr-profile="$COV_DIR/merged.profdata" \
	--object="$TEST_BIN" \
	--object="$INSTALLED_DYLIB" \
	--ignore-filename-regex='/.cargo/registry/|/vendor/|/rustc/|/rustlib/|/.rustup/|/target/debug/build/'

exit "$pgtap_status"
