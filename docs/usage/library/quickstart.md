# Quickstart

Install browser-use, pick an LLM, and run a first agent.

> Upstream library documentation, kept in `docs/` so the router files stay thin.

## Quickstart
To get started with Browser Use you need to install the package and create an `.env` file with your API key.

<Note icon="key" color="#FFC107" iconType="regular">
  `ChatBrowserUse` offers the [fastest and most cost-effective models](https://browser-use.com/posts/speed-matters/), completing tasks 3-5x faster. Get your API key at [cloud.browser-use.com](https://cloud.browser-use.com/new-api-key).
</Note>

### 1. Installing Browser-Use

```bash create environment theme={null}
pip install uv
uv venv --python 3.12
```

```bash activate environment theme={null}
source .venv/bin/activate
# On Windows use `.venv\Scripts\activate`
```

```bash install browser-use & chromium theme={null}
uv pip install browser-use
uvx browser-use install
```

### 2. Choose your favorite LLM

Create a `.env` file and add your API key.

<Callout icon="key" iconType="regular">
  We recommend using ChatBrowserUse which is optimized for browser automation tasks (highest accuracy + fastest speed + lowest token cost). Get your API key [here](https://cloud.browser-use.com/new-api-key).
</Callout>

```bash .env theme={null}
touch .env
```

<Info>On Windows, use `echo. > .env`</Info>

Then add your API key to the file.

<CodeGroup>
  ```bash Browser Use theme={null}
  # add your key to .env file
  BROWSER_USE_API_KEY=
  # Get your API key at https://cloud.browser-use.com/new-api-key
  ```

  ```bash Google theme={null}
  # add your key to .env file
  GOOGLE_API_KEY=
  # Get your free Gemini API key from https://aistudio.google.com/app/u/1/apikey?pli=1.
  ```

  ```bash OpenAI theme={null}
  # add your key to .env file
  OPENAI_API_KEY=
  ```

  ```bash Anthropic theme={null}
  # add your key to .env file
  ANTHROPIC_API_KEY=
  ```
</CodeGroup>

See [Supported Models](https://docs.browser-use.com/supported-models#supported-models) for more.

### 3. Run your first agent

<CodeGroup>
  ```python Browser Use theme={null}
  from browser_use import Agent, ChatBrowserUse
  from dotenv import load_dotenv
  import asyncio

  load_dotenv()

  async def main():
      llm = ChatBrowserUse()
      task = "Find the number 1 post on Show HN"
      agent = Agent(task=task, llm=llm)
      await agent.run()

  if __name__ == "__main__":
      asyncio.run(main())
  ```

  ```python Google theme={null}
  from browser_use import Agent, ChatGoogle
  from dotenv import load_dotenv
  import asyncio

  load_dotenv()

  async def main():
      llm = ChatGoogle(model="gemini-3-flash-preview")
      task = "Find the number 1 post on Show HN"
      agent = Agent(task=task, llm=llm)
      await agent.run()

  if __name__ == "__main__":
      asyncio.run(main())
  ```

  ```python OpenAI theme={null}
  from browser_use import Agent, ChatOpenAI
  from dotenv import load_dotenv
  import asyncio

  load_dotenv()

  async def main():
      llm = ChatOpenAI(model="gpt-4.1-mini")
      task = "Find the number 1 post on Show HN"
      agent = Agent(task=task, llm=llm)
      await agent.run()

  if __name__ == "__main__":
      asyncio.run(main())
  ```

  ```python Anthropic theme={null}
  from browser_use import Agent, ChatAnthropic
  from dotenv import load_dotenv
  import asyncio

  load_dotenv()

  async def main():
      llm = ChatAnthropic(model='claude-sonnet-4-0', temperature=0.0)
      task = "Find the number 1 post on Show HN"
      agent = Agent(task=task, llm=llm)
      await agent.run()

  if __name__ == "__main__":
      asyncio.run(main())
  ```
</CodeGroup>

<Note> Custom browsers can be configured in one line. Check out <a href="https://docs.browser-use.com/customize/browser/basics">browsers</a> for more. </Note>

### 4. Going to Production

Sandboxes are the **easiest way to run Browser-Use in production**. We handle agents, browsers, persistence, auth, cookies, and LLMs. It's also the **fastest way to deploy** - the agent runs right next to the browser, so latency is minimal.

To run in production with authentication, just add `@sandbox` to your function:

```python  theme={null}
from browser_use import Browser, sandbox, ChatBrowserUse
from browser_use.agent.service import Agent
import asyncio

@sandbox(cloud_profile_id='your-profile-id')
async def production_task(browser: Browser):
    agent = Agent(task="Your authenticated task", browser=browser, llm=ChatBrowserUse())
    await agent.run()

asyncio.run(production_task())
```

See [Going to Production](https://docs.browser-use.com/production) for how to sync your cookies to the cloud.
