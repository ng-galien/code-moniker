#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
EXT_DIR="$ROOT_DIR/vscode-extension"

usage() {
	cat <<'EOF'
Usage: ./scripts/install-vscode-extension.sh [--skip-cli] [--no-deps]

Build and install the Code Moniker VS Code extension from source.

Options:
  --skip-cli  Do not install the code-moniker CLI with cargo.
  --no-deps   Do not install npm dependencies when node_modules is missing.
  --skip-tests
             Do not run sample/notebook validation before packaging.
  -h, --help  Show this help.
EOF
}

skip_cli=false
install_deps=true
run_tests=true

while [[ $# -gt 0 ]]; do
	case "$1" in
		--skip-cli)
			skip_cli=true
			shift
			;;
		--no-deps)
			install_deps=false
			shift
			;;
		--skip-tests)
			run_tests=false
			shift
			;;
		-h|--help)
			usage
			exit 0
			;;
		*)
			echo "Unknown option: $1" >&2
			usage >&2
			exit 2
			;;
	esac
done

need_command() {
	if ! command -v "$1" >/dev/null 2>&1; then
		echo "Missing required command: $1" >&2
		exit 1
	fi
}

need_command npm
need_command npx
need_command code

if [[ "$skip_cli" == false ]]; then
	need_command cargo
	echo "Installing code-moniker CLI..."
	cargo install --path "$ROOT_DIR/crates/cli"
fi

if [[ ! -d "$EXT_DIR/node_modules" ]]; then
	if [[ "$install_deps" == false ]]; then
		echo "Missing $EXT_DIR/node_modules; rerun without --no-deps." >&2
		exit 1
	fi
	echo "Installing VS Code extension dependencies..."
	(cd "$EXT_DIR" && npm ci)
fi

if [[ "$run_tests" == true ]]; then
	need_command cargo
	echo "Validating executable sample catalog..."
	(cd "$ROOT_DIR" && cargo test -p code-moniker --test samples_contract)

	echo "Validating VS Code extension catalog metadata..."
	(cd "$EXT_DIR" && npm run validate)

	echo "Typechecking VS Code extension..."
	(cd "$EXT_DIR" && npm run typecheck)

	echo "Running VS Code notebook integration tests..."
	(cd "$EXT_DIR" && npm run test:integration)
fi

echo "Compiling VS Code extension..."
(cd "$EXT_DIR" && npm run compile)

echo "Packaging VS Code extension..."
vsix_path="$(
	cd "$EXT_DIR"
	npx --yes @vscode/vsce package --out code-moniker.vsix >/dev/null
	printf '%s\n' "$EXT_DIR/code-moniker.vsix"
)"

echo "Installing $vsix_path..."
code --install-extension "$vsix_path" --force

echo "Installed Code Moniker VS Code extension."
echo "Reload VS Code windows that already had Code Moniker loaded: run 'Developer: Reload Window'."
