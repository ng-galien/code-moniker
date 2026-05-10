#!/usr/bin/env bash
# Run pgTAP test suite against the pgrx-managed PostgreSQL.
#
# Workflow:
#   1. Ensure the pgrx PG instance is running (`cargo pgrx start pg17`).
#   2. (Re)install the extension (`cargo pgrx install`).
#   3. Drop and recreate a fresh test DB.
#   4. Execute each test/sql/*.sql file with psql, surfacing TAP output.
#
# Exit code is non-zero on any of the failure modes pgTAP can surface:
#   - "not ok …"                     individual test failed
#   - "# Looks like you planned …"   plan() smaller/larger than run count
#   - "# Looks like you failed …"    rollup line emitted when ≥1 test failed
#   - "Dubious"                      prove-harness convention; cheap to also
#                                    catch in case of nested runners
# A non-zero psql exit (ON_ERROR_STOP triggered, e.g. an ERROR raised
# outside any pgtap `throws_*` assertion) is also fatal.

set -euo pipefail

PG_VERSION="${PG_VERSION:-pg17}"
PG_CONFIG="${PG_CONFIG:-$HOME/.pgrx/17.9/pgrx-install/bin/pg_config}"
PG_BIN="$(dirname "$PG_CONFIG")"
PG_PORT="${PG_PORT:-28817}"
DB_NAME="${DB_NAME:-pg_code_moniker_test}"

PSQL="$PG_BIN/psql -h localhost -p $PG_PORT -X -q -A -t -v ON_ERROR_STOP=1"

# Recreate the test DB.
$PSQL -d postgres -c "DROP DATABASE IF EXISTS $DB_NAME;" >/dev/null
$PSQL -d postgres -c "CREATE DATABASE $DB_NAME;" >/dev/null

# Run every test/sql/*.sql in lexical order. Each file must emit its own
# plan() … finish() block.
TEST_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/sql"
fail=0
for f in "$TEST_DIR"/*.sql; do
	echo "# ${f##*/}"
	if ! output=$($PSQL -d "$DB_NAME" -f "$f" 2>&1); then
		echo "$output"
		echo "# FAIL ${f##*/} (psql exit non-zero)"
		fail=1
		continue
	fi
	echo "$output"
	file_fail=0
	if grep -qE '^[[:space:]]*not ok' <<<"$output"; then
		file_fail=1
	fi
	if grep -qE '^# Looks like you planned' <<<"$output"; then
		echo "# FAIL ${f##*/} (plan/run mismatch)"
		file_fail=1
	fi
	if grep -qE '^# Looks like you failed' <<<"$output"; then
		echo "# FAIL ${f##*/} (failure rollup)"
		file_fail=1
	fi
	if grep -qE '^# Failed test\b' <<<"$output"; then
		echo "# FAIL ${f##*/} (per-assert failure)"
		file_fail=1
	fi
	if grep -qE '\bDubious\b' <<<"$output"; then
		echo "# FAIL ${f##*/} (dubious harness state)"
		file_fail=1
	fi
	if [ "$file_fail" -ne 0 ]; then
		fail=1
	fi
done

exit $fail
