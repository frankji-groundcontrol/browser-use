# Tools

The action registry: basics, adding custom tools, the built-in set, removing tools, and tool responses.

> Upstream library documentation, kept in `docs/` so the router files stay thin.

## Tools: Basics
Source: (go to or request this content to learn more) https://docs.browser-use.com/customize/tools/basics
Tools are the functions that the agent has to interact with the world.

### Quick Example

```python  theme={null}
from browser_use import Tools, ActionResult, BrowserSession

tools = Tools()

@tools.action('Ask human for help with a question')
async def ask_human(question: str, browser_session: BrowserSession) -> ActionResult:
    answer = input(f'{question} > ')
    return ActionResult(extracted_content=f'The human responded with: {answer}')

agent = Agent(
    task='Ask human for help',
    llm=llm,
    tools=tools,
)
```

<Warning>
**Important**: The parameter must be named exactly `browser_session` with type `BrowserSession` (not `browser: Browser`). 
The agent injects parameters by name matching, so using the wrong name will cause your tool to fail silently.
</Warning>

<Note>
  Use `browser_session` parameter in tools for deterministic [Actor](https://docs.browser-use.com/legacy/actor/basics) actions.
</Note>



## Tools: Add Tools
Source: (go to or request this content to learn more) https://docs.browser-use.com/customize/tools/add

Examples:
* deterministic clicks
* file handling
* calling APIs
* human-in-the-loop
* browser interactions
* calling LLMs
* get 2fa codes
* send emails
* Playwright integration (see [GitHub example](https://github.com/browser-use/browser-use/blob/main/examples/browser/playwright_integration.py))
* ...

Simply add `@tools.action(...)` to your function.

```python  theme={null}
from browser_use import Tools, Agent, ActionResult

tools = Tools()

@tools.action(description='Ask human for help with a question')
async def ask_human(question: str) -> ActionResult:
    answer = input(f'{question} > ')
    return ActionResult(extracted_content=f'The human responded with: {answer}')
```

```python  theme={null}
agent = Agent(task='...', llm=llm, tools=tools)
```

* `description` *(required)* - What the tool does, the LLM uses this to decide when to call it.
* `allowed_domains` - List of domains where tool can run (e.g. `['*.example.com']`), defaults to all domains

The Agent fills your function parameters based on their names, type hints, & defaults.

<Warning>
**Common Pitfall**: Parameter names must match exactly! Use `browser_session: BrowserSession` (not `browser: Browser`). 
The agent injects special parameters by **name matching**, so using incorrect names will cause your tool to fail silently.
</Warning>


## Tools: Available Tools
Source: (go to or request this content to learn more) https://docs.browser-use.com/customize/tools/available
Here is the [source code](https://github.com/browser-use/browser-use/blob/main/browser_use/tools/service.py) for the default tools:

#### Navigation & Browser Control

* `search` - Search queries (DuckDuckGo, Google, Bing)
* `navigate` - Navigate to URLs
* `go_back` - Go back in browser history
* `wait` - Wait for specified seconds

#### Page Interaction

* `click` - Click elements by their index
* `input` - Input text into form fields
* `upload_file` - Upload files to file inputs
* `scroll` - Scroll the page up/down
* `find_text` - Scroll to specific text on page
* `send_keys` - Send special keys (Enter, Escape, etc.)

#### JavaScript Execution

* `evaluate` - Execute custom JavaScript code on the page (for advanced interactions, shadow DOM, custom selectors, data extraction)

#### Tab Management

* `switch` - Switch between browser tabs
* `close` - Close browser tabs

#### Content Extraction

* `extract` - Extract data from webpages using LLM

#### Visual Analysis

* `screenshot` - Request a screenshot in your next browser state for visual confirmation

#### Form Controls

* `dropdown_options` - Get dropdown option values
* `select_dropdown` - Select dropdown options

#### File Operations

* `write_file` - Write content to files
* `read_file` - Read file contents
* `replace_file` - Replace text in files

#### Task Completion

* `done` - Complete the task (always available)



## Tools: Remove Tools
Source: (go to or request this content to learn more) https://docs.browser-use.com/customize/tools/remove

You can exclude default tools:

```python  theme={null}
from browser_use import Tools

tools = Tools(exclude_actions=['search', 'wait'])
agent = Agent(task='...', llm=llm, tools=tools)
```


## Tools: Tool Response
Source: (go to or request this content to learn more) https://docs.browser-use.com/customize/tools/response
Tools return results using `ActionResult` or simple strings.

### Return Types

```python  theme={null}
@tools.action('My tool')
def my_tool() -> str:
    return "Task completed successfully"

@tools.action('Advanced tool')
def advanced_tool() -> ActionResult:
    return ActionResult(
        extracted_content="Main result",
        long_term_memory="Remember this info",
        error="Something went wrong",
        is_done=True,
        success=True,
        attachments=["file.pdf"],
    )
```
