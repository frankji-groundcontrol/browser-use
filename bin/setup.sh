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

echo "[+] Installing uv..."
curl -LsSf https://astral.sh/uv/install.sh | sh
# The installer cannot update the parent shell's PATH.
export PATH="$HOME/.local/bin:$PATH"

#git checkout main git pull
echo
echo "[+] Setting up venv"
uv venv
echo
echo "[+] Installing packages in venv"
uv sync --dev --all-extras
echo
echo "[i] Tip: make sure to set BROWSER_USE_LOGGING_LEVEL=debug and your LLM API keys in your .env file"
echo
uv pip show browser-use

echo "Usage:"
echo "  $ browser-use               use the CLI"
echo "  or"
echo "  $ source .venv/bin/activate"
echo "  $ ipython                   use the library"
echo "  >>> from browser_use import BrowserSession, Agent"
echo "  >>> await Agent(task='book me a flight to fiji', browser=BrowserSession(headless=False)).run()"
echo ""
