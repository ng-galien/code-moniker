#!/usr/bin/env bash
# Run pgTAP test suite against the pgrx-managed PostgreSQL.
#
# Workflow:
#   1. Ensure the pgrx PG instance is running (`cargo pgrx start pg17`).
#   2. (Re)install the extension (`cargo pgrx install`).
#   3. Drop and recreate a fresh test DB.
#   4. Execute each test/sql/*.sql file with psql, surfacing TAP output.
#
# Exit code is non-zero on any "not ok" line or psql failure.

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
	if grep -qE '^[[:space:]]*not ok' <<<"$output"; then
		fail=1
	fi
done

exit $fail
