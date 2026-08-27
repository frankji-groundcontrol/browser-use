# Going to production

Deployment, proxies for stealth, and syncing local cookies to the cloud.

> Upstream library documentation, kept in `docs/` so the router files stay thin.

## Going to Production

> Deploy your local Browser-Use code to production with `@sandbox` wrapper, and scale to millions of agents

### 1. Basic Deployment

Wrap your existing local code with `@sandbox()`:

```python  theme={null}
from browser_use import Browser, sandbox, ChatBrowserUse
from browser_use.agent.service import Agent
import asyncio

@sandbox()
async def my_task(browser: Browser):
    agent = Agent(task="Find the top HN post", browser=browser, llm=ChatBrowserUse())
    await agent.run()

# Just call it like any async function
asyncio.run(my_task())
```

That's it - your code now runs in production at scale. We handle agents, browsers, persistence, and LLMs.

### 2. Add Proxies for Stealth

Use country-specific proxies to bypass captchas, Cloudflare, and geo-restrictions:

```python  theme={null}
@sandbox(cloud_proxy_country_code='us')  # Route through US proxy
async def stealth_task(browser: Browser):
    agent = Agent(task="Your task", browser=browser, llm=ChatBrowserUse())
    await agent.run()
```

### 3. Sync Local Cookies to Cloud

To use your local authentication in production:

**First**, create an API key at [cloud.browser-use.com/new-api-key](https://cloud.browser-use.com/new-api-key) or follow the instruction on [Cloud - Profiles](https://cloud.browser-use.com/dashboard/settings?tab=profiles)

**Then**, install `profile-use` for your platform from the [official releases](https://github.com/browser-use/profile-use-releases/releases/latest) and follow the [profile sync guide](https://github.com/browser-use/browser-harness/blob/main/interaction-skills/profile-sync.md) to sync your local cookies.

This opens a browser where you log into your accounts. You'll get a `profile_id`.

**Finally**, use it in production:

```python  theme={null}
@sandbox(cloud_profile_id='your-profile-id')
async def authenticated_task(browser: Browser):
    agent = Agent(task="Your authenticated task", browser=browser, llm=ChatBrowserUse())
    await agent.run()
```

Your cloud browser is already logged in!

***

For more sandbox parameters and events, see [Sandbox Quickstart](https://docs.browser-use.com/legacy/sandbox/quickstart).
