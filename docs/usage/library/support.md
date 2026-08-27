# Help and telemetry

Where to get help, and what anonymous telemetry collects (and how to disable it).

> Upstream library documentation, kept in `docs/` so the router files stay thin.

## Get Help
Source: (go to or request this content to learn more) https://docs.browser-use.com/development/get-help

More than 20k developers help each other

1. Check our [GitHub Issues](https://github.com/browser-use/browser-use/issues)
2. Ask in our [Discord community](https://link.browser-use.com/discord)
3. Get support for your enterprise with [support@browser-use.com](mailto:support@browser-use.com)

## Telemetry
Source: (go to or request this content to learn more) https://docs.browser-use.com/development/monitoring/telemetry
Understanding Browser Use's telemetry

### Overview

Browser Use is free under the MIT license. To help us continue improving the library, we collect anonymous usage data with [PostHog](https://posthog.com) . This information helps us understand how the library is used, fix bugs more quickly, and prioritize new features.

### Opting Out

You can disable telemetry by setting the environment variable:

```bash .env theme={null}
ANONYMIZED_TELEMETRY=false
```

Or in your Python code:

```python  theme={null}
import os
os.environ["ANONYMIZED_TELEMETRY"] = "false"
```

<Note>
  Even when enabled, telemetry has zero impact on the library's performance. Code is available in [Telemetry
  Service](https://github.com/browser-use/browser-use/tree/main/browser_use/telemetry).
</Note>
