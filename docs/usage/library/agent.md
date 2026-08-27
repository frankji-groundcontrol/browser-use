# Agent

The `Agent` class: basics, every parameter, output format, prompting, and supported models.

> Upstream library documentation, kept in `docs/` so the router files stay thin.

## Agent Basics
```python  theme={null}
from browser_use import Agent, ChatBrowserUse

agent = Agent(
    task="Search for latest news about AI",
    llm=ChatBrowserUse(),
)

async def main():
    history = await agent.run(max_steps=100)
```

* `task`: The task you want to automate.
* `llm`: Your favorite LLM. See <a href="https://docs.browser-use.com/customize/agent/supported-models">Supported Models</a>.

The agent is executed using the async `run()` method:

* `max_steps` (default: `100`): Maximum number of steps an agent can take.

Check out all customizable parameters <a href="https://docs.browser-use.com/customize/agent/all-parameters"> here</a>.

## Agent All Parameters
> Complete reference for all agent configuration options

### Available Parameters

#### Core Settings

* `tools`: Registry of <a href="https://docs.browser-use.com/customize/tools/available">tools</a> the agent can call. <a href="https://docs.browser-use.com/customize/tools/basics">Example</a>
* `browser`: Browser object where you can specify the browser settings.
* `output_model_schema`: Pydantic model class for structured output validation. [Example](https://github.com/browser-use/browser-use/blob/main/examples/features/custom_output.py)

#### Vision & Processing

* `use_vision` (default: `"auto"`): Vision mode - `"auto"` includes screenshot tool but only uses vision when requested, `True` always includes screenshots, `False` never includes screenshots and excludes screenshot tool
* `vision_detail_level` (default: `'auto'`): Screenshot detail level - `'low'`, `'high'`, or `'auto'`
* `page_extraction_llm`: Separate LLM model for page content extraction. You can choose a small & fast model because it only needs to extract text from the page (default: same as `llm`)

#### Actions & Behavior

* `initial_actions`: List of actions to run before the main task without LLM. [Example](https://github.com/browser-use/browser-use/blob/main/examples/features/initial_actions.py)
* `max_actions_per_step` (default: `3`): Maximum actions per step, e.g. for form filling the agent can output 3 fields at once. We execute the actions until the page changes.
* `max_failures` (default: `3`): Maximum retries for steps with errors
* `final_response_after_failure` (default: `True`): If True, attempt to force one final model call with intermediate output after max\_failures is reached
* `use_thinking` (default: `True`): Controls whether the agent uses its internal "thinking" field for explicit reasoning steps.
* `flash_mode` (default: `False`): Fast mode that skips evaluation, next goal and thinking and only uses memory. If `flash_mode` is enabled, it overrides `use_thinking` and disables the thinking process entirely. [Example](https://github.com/browser-use/browser-use/blob/main/examples/getting_started/05_fast_agent.py)

#### System Messages

* `override_system_message`: Completely replace the default system prompt.
* `extend_system_message`: Add additional instructions to the default system prompt. [Example](https://github.com/browser-use/browser-use/blob/main/examples/features/custom_system_prompt.py)

#### File & Data Management

* `save_conversation_path`: Path to save complete conversation history
* `save_conversation_path_encoding` (default: `'utf-8'`): Encoding for saved conversations
* `available_file_paths`: List of file paths the agent can access
* `sensitive_data`: Dictionary of sensitive data to handle carefully. [Example](https://github.com/browser-use/browser-use/blob/main/examples/features/sensitive_data.py)

#### Visual Output

* `generate_gif` (default: `False`): Generate GIF of agent actions. Set to `True` or string path
* `include_attributes`: List of HTML attributes to include in page analysis

#### Performance & Limits

* `max_history_items`: Maximum number of last steps to keep in the LLM memory. If `None`, we keep all steps.
* `llm_timeout` (default: `90`): Timeout in seconds for LLM calls
* `step_timeout` (default: `120`): Timeout in seconds for each step
* `directly_open_url` (default: `True`): If we detect a url in the task, we directly open it.

#### Advanced Options

* `calculate_cost` (default: `False`): Calculate and track API costs
* `display_files_in_done_text` (default: `True`): Show file information in completion messages

#### Backwards Compatibility

* `controller`: Alias for `tools` for backwards compatibility.
* `browser_session`: Alias for `browser` for backwards compatibility.

## Agent Output Format

### Agent History

The `run()` method returns an `AgentHistoryList` object with the complete execution history:

```python  theme={null}
history = await agent.run()

# Access useful information
history.urls()                    # List of visited URLs
history.screenshot_paths()        # List of screenshot paths
history.screenshots()             # List of screenshots as base64 strings
history.action_names()            # Names of executed actions
history.extracted_content()       # List of extracted content from all actions
history.errors()                  # List of errors (with None for steps without errors)
history.model_actions()           # All actions with their parameters
history.model_outputs()           # All model outputs from history
history.last_action()             # Last action in history

# Analysis methods
history.final_result()            # Get the final extracted content (last step)
history.is_done()                 # Check if agent completed successfully
history.is_successful()           # Check if agent completed successfully (returns None if not done)
history.has_errors()              # Check if any errors occurred
history.model_thoughts()          # Get the agent's reasoning process (AgentBrain objects)
history.action_results()          # Get all ActionResult objects from history
history.action_history()          # Get truncated action history with essential fields
history.number_of_steps()         # Get the number of steps in the history
history.total_duration_seconds()  # Get total duration of all steps in seconds

# Structured output (when using output_model_schema)
history.structured_output         # Property that returns parsed structured output
```

See all helper methods in the [AgentHistoryList source code](https://github.com/browser-use/browser-use/blob/main/browser_use/agent/views.py#L301).

### Structured Output

For structured output, use the `output_model_schema` parameter with a Pydantic model. [Example](https://github.com/browser-use/browser-use/blob/main/examples/features/custom_output.py).

### Agent History

The `run()` method returns an `AgentHistoryList` object with the complete execution history:

```python  theme={null}
history = await agent.run()

# Access useful information
history.urls()                    # List of visited URLs
history.screenshot_paths()        # List of screenshot paths
history.screenshots()             # List of screenshots as base64 strings
history.action_names()            # Names of executed actions
history.extracted_content()       # List of extracted content from all actions
history.errors()                  # List of errors (with None for steps without errors)
history.model_actions()           # All actions with their parameters
history.model_outputs()           # All model outputs from history
history.last_action()             # Last action in history

# Analysis methods
history.final_result()            # Get the final extracted content (last step)
history.is_done()                 # Check if agent completed successfully
history.is_successful()           # Check if agent completed successfully (returns None if not done)
history.has_errors()              # Check if any errors occurred
history.model_thoughts()          # Get the agent's reasoning process (AgentBrain objects)
history.action_results()          # Get all ActionResult objects from history
history.action_history()          # Get truncated action history with essential fields
history.number_of_steps()         # Get the number of steps in the history
history.total_duration_seconds()  # Get total duration of all steps in seconds

# Structured output (when using output_model_schema)
history.structured_output         # Property that returns parsed structured output
```

See all helper methods in the [AgentHistoryList source code](https://github.com/browser-use/browser-use/blob/main/browser_use/agent/views.py#L301).

### Structured Output

For structured output, use the `output_model_schema` parameter with a Pydantic model. [Example](https://github.com/browser-use/browser-use/blob/main/examples/features/custom_output.py).


## Agent Prompting Guide
> Tips and tricks

Prompting can drastically improve performance and solve existing limitations of the library.

#### 1. Be Specific vs Open-Ended

**✅ Specific (Recommended)**

```python  theme={null}
task = """
1. Go to https://quotes.toscrape.com/
2. Use extract action with the query "first 3 quotes with their authors"
3. Save results to quotes.csv using write_file action
4. Do a google search for the first quote and find when it was written
"""
```

**❌ Open-Ended**

```python  theme={null}
task = "Go to web and make money"
```

#### 2. Name Actions Directly

When you know exactly what the agent should do, reference actions by name:

```python  theme={null}
task = """
1. Use search action to find "Python tutorials"
2. Use click to open first result in a new tab
3. Use scroll action to scroll down 2 pages
4. Use extract to extract the names of the first 5 items
5. Wait for 2 seconds if the page is not loaded, refresh it and wait 10 sec
6. Use send_keys action with "Tab Tab ArrowDown Enter"
"""
```

See [Available Tools](https://docs.browser-use.com/customize/tools/available) for the complete list of actions.

#### 3. Handle interaction problems via keyboard navigation

Sometimes buttons can't be clicked (you found a bug in the library - open an issue).
Good news - often you can work around it with keyboard navigation!

```python  theme={null}
task = """
If the submit button cannot be clicked:
1. Use send_keys action with "Tab Tab Enter" to navigate and activate
2. Or use send_keys with "ArrowDown ArrowDown Enter" for form submission
"""
```

#### 4. Custom Actions Integration

```python  theme={null}
# When you have custom actions
@controller.action("Get 2FA code from authenticator app")
async def get_2fa_code():
    # Your implementation
    pass

task = """
Login with 2FA:
1. Enter username/password
2. When prompted for 2FA, use get_2fa_code action
3. NEVER try to extract 2FA codes from the page manually
4. ALWAYS use the get_2fa_code action for authentication codes
"""
```

#### 5. Error Recovery

```python  theme={null}
task = """
Robust data extraction:
1. Go to openai.com to find their CEO
2. If navigation fails due to anti-bot protection:
   - Use google search to find the CEO
3. If page times out, use go_back and try alternative approach
"""
```

The key to effective prompting is being specific about actions.


## Agent Supported Models
Source: (go to or request this content to learn more) https://docs.browser-use.com/customize/agent/supported-models
LLMs supported (changes frequently, check the documentation when needed)
Most recommended LLM is the ChatBrowserUse chat api.
