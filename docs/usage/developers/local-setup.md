# Local setup from source

Install from a git checkout, configure the environment, and run pre-commit plus the CI suite.

> Upstream library documentation, kept in `docs/` so the router files stay thin.

## Local Setup
Source: (go to or request this content to learn more) https://docs.browser-use.com/development/setup/local-setup

We're excited to have you join our community of contributors.
### Welcome to Browser Use Development!

```bash  theme={null}
git clone https://github.com/browser-use/browser-use
cd browser-use
uv sync --all-extras --dev
# or pip install -U git+https://github.com/browser-use/browser-use.git@main
```

### Configuration
Set up your environment variables:

```bash  theme={null}
# Copy the example environment file
cp .env.example .env

# set logging level
# BROWSER_USE_LOGGING_LEVEL=debug
```

### Helper Scripts

For common development tasks

```bash  theme={null}
# Complete setup script - installs uv, creates a venv, and installs dependencies
./bin/setup.sh

# Run all pre-commit hooks (formatting, linting, type checking)
./bin/lint.sh

# Run the core test suite that's executed in CI
./bin/test.sh
```

### Run examples

```bash  theme={null}
uv run examples/simple.py
```
