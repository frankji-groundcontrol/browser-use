#!/usr/bin/env bash
# This script is used to setup a local development environment for the browser-use project.
# Usage:
#   $ ./bin/setup.sh

### Bash Environment Setup
# http://redsymbol.net/articles/unofficial-bash-strict-mode/
# https://www.gnu.org/software/bash/manual/html_node/The-Set-Builtin.html
# set -o xtrace
# set -x
# shopt -s nullglob
set -o errexit
set -o errtrace
set -o nounset
set -o pipefail
IFS=$'\n'

SCRIPT_DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" >/dev/null 2>&1 && pwd )"
REPO_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_DIR"


if [ -f "$SCRIPT_DIR/lint.sh" ]; then
    echo "[√] already inside a cloned browser-use repo"
else
    echo "[+] Cloning browser-use repo into current directory: $REPO_DIR"
    git clone https://github.com/browser-use/browser-use "$REPO_DIR/browser-use"
    cd "$REPO_DIR/browser-use"
fi

if ! command -v uv >/dev/null 2>&1; then
    echo "[+] Installing uv..."
    curl -LsSf https://astral.sh/uv/install.sh | sh
    export PATH="$HOME/.local/bin:$PATH"
fi

#git checkout main git pull
echo
echo "[+] Setting up venv"
export UV_PROJECT_ENVIRONMENT="$PWD/.venv"
if [ ! -x "$UV_PROJECT_ENVIRONMENT/bin/python" ]; then
    uv venv --python "$(cat .python-version)" "$UV_PROJECT_ENVIRONMENT"
fi
source "$UV_PROJECT_ENVIRONMENT/bin/activate"
echo
echo "[+] Installing packages in venv"
uv sync --locked --dev --all-extras
echo
echo "[i] Tip: make sure to set BROWSER_USE_LOGGING_LEVEL=debug and your LLM API keys in your .env file"
echo
uv pip show --python "$UV_PROJECT_ENVIRONMENT/bin/python" browser-use
uv run --no-sync python -c 'import os, pathlib, sys; assert pathlib.Path(sys.prefix).resolve() == pathlib.Path(os.environ["UV_PROJECT_ENVIRONMENT"]).resolve(); import browser_use; print("Verified browser_use in the project environment")'

echo "Usage:"
echo "  $ browser-use               use the CLI"
echo "  or"
echo "  $ source .venv/bin/activate"
echo "  $ ipython                   use the library"
echo "  >>> from browser_use import BrowserSession, Agent"
echo "  >>> await Agent(task='book me a flight to fiji', browser=BrowserSession(headless=False)).run()"
echo ""
