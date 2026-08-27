# Browser

The `Browser` class: basics, every parameter, connecting to a real local Chrome, and remote/cloud browsers.

> Upstream library documentation, kept in `docs/` so the router files stay thin.

## Browser Basics

```python  theme={null}
from browser_use import Agent, Browser, ChatBrowserUse

browser = Browser(
	headless=False,  # Show browser window
	window_size={'width': 1000, 'height': 700},  # Set window size
)

agent = Agent(
	task='Search for Browser Use',
	browser=browser,
	llm=ChatBrowserUse(),
)


async def main():
	await agent.run()
```

## Browser All Parameters
> Complete reference for all browser configuration options

<Note>
  The `Browser` instance also provides all [Actor](https://docs.browser-use.com/legacy/actor/all-parameters) methods for direct browser control (page management, element interactions, etc.).
</Note>

### Core Settings

* `cdp_url`: CDP URL for connecting to existing browser instance (e.g., `"http://localhost:9222"`)

### Display & Appearance

* `headless` (default: `None`): Run browser without UI. Auto-detects based on display availability (`True`/`False`/`None`)
* `window_size`: Browser window size for headful mode. Use dict `{'width': 1920, 'height': 1080}` or `ViewportSize` object
* `window_position` (default: `{'width': 0, 'height': 0}`): Window position from top-left corner in pixels
* `viewport`: Content area size, same format as `window_size`. Use `{'width': 1280, 'height': 720}` or `ViewportSize` object
* `no_viewport` (default: `None`): Disable viewport emulation, content fits to window size
* `device_scale_factor`: Device scale factor (DPI). Set to `2.0` or `3.0` for high-resolution screenshots

### Browser Behavior

* `keep_alive` (default: `None`): Keep browser running after agent completes
* `allowed_domains`: Restrict navigation to specific domains. Domain pattern formats:
  * `'example.com'` - Matches only `https://example.com/*`
  * `'*.example.com'` - Matches `https://example.com/*` and any subdomain `https://*.example.com/*`
  * `'http*://example.com'` - Matches both `http://` and `https://` protocols
  * `'chrome-extension://*'` - Matches any Chrome extension URL
  * **Security**: Wildcards in TLD (e.g., `example.*`) are **not allowed** for security
  * Use list like `['*.google.com', 'https://example.com', 'chrome-extension://*']`
  * **Performance**: Lists with 100+ domains are automatically optimized to sets for O(1) lookup. Pattern matching is disabled for optimized lists. Both `www.example.com` and `example.com` variants are checked automatically.
* `prohibited_domains`: Block navigation to specific domains. Uses same pattern formats as `allowed_domains`. When both `allowed_domains` and `prohibited_domains` are set, `allowed_domains` takes precedence. Examples:
  * `['pornhub.com', '*.gambling-site.net']` - Block specific sites and all subdomains
  * `['https://explicit-content.org']` - Block specific protocol/domain combination
  * **Performance**: Lists with 100+ domains are automatically optimized to sets for O(1) lookup (same as `allowed_domains`)
* `enable_default_extensions` (default: `True`): Load automation extensions (uBlock Origin, cookie handlers, ClearURLs)
* `cross_origin_iframes` (default: `False`): Enable cross-origin iframe support (may cause complexity)
* `is_local` (default: `True`): Whether this is a local browser instance. Set to `False` for remote browsers. If we have a `executable_path` set, it will be automatically set to `True`. This can effect your download behavior.

### User Data & Profiles

* `user_data_dir` (default: auto-generated temp): Directory for browser profile data. Use `None` for incognito mode
* `profile_directory` (default: `'Default'`): Chrome profile subdirectory name (`'Profile 1'`, `'Work Profile'`, etc.)
* `storage_state`: Browser storage state (cookies, localStorage). Can be file path string or dict object

### Network & Security

* `proxy`: Proxy configuration using `ProxySettings(server='http://host:8080', bypass='localhost,127.0.0.1', username='user', password='pass')`

* `permissions` (default: `['clipboardReadWrite', 'notifications']`): Browser permissions to grant. Use list like `['camera', 'microphone', 'geolocation']`

* `headers`: Additional HTTP headers for connect requests (remote browsers only)

### Browser Launch

* `executable_path`: Path to browser executable for custom installations. Platform examples:
  * macOS: `'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'`
  * Windows: `'C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe'`
  * Linux: `'/usr/bin/google-chrome'`
* `channel`: Browser channel (`'chromium'`, `'chrome'`, `'chrome-beta'`, `'msedge'`, etc.)
* `args`: Additional command-line arguments for the browser. Use list format: `['--disable-gpu', '--custom-flag=value', '--another-flag']`
* `env`: Environment variables for browser process. Use dict like `{'DISPLAY': ':0', 'LANG': 'en_US.UTF-8', 'CUSTOM_VAR': 'test'}`
* `chromium_sandbox` (default: `True` except in Docker): Enable Chromium sandboxing for security
* `devtools` (default: `False`): Open DevTools panel automatically (requires `headless=False`)
* `ignore_default_args`: List of default args to disable, or `True` to disable all. Use list like `['--enable-automation', '--disable-extensions']`

### Timing & Performance

* `minimum_wait_page_load_time` (default: `0.25`): Minimum time to wait before capturing page state in seconds
* `wait_for_network_idle_page_load_time` (default: `0.5`): Time to wait for network activity to cease in seconds
* `wait_between_actions` (default: `0.5`): Time to wait between agent actions in seconds

### AI Integration

* `highlight_elements` (default: `True`): Highlight interactive elements for AI vision
* `paint_order_filtering` (default: `True`): Enable paint order filtering to optimize DOM tree by removing elements hidden behind others. Slightly experimental

### Downloads & Files

* `accept_downloads` (default: `True`): Automatically accept all downloads
* `downloads_path`: Directory for downloaded files. Use string like `'./downloads'` or `Path` object
* `auto_download_pdfs` (default: `True`): Automatically download PDFs instead of viewing in browser

### Device Emulation

* `user_agent`: Custom user agent string. Example: `'Mozilla/5.0 (iPhone; CPU iPhone OS 14_0 like Mac OS X)'`
* `screen`: Screen size information, same format as `window_size`

### Recording & Debugging

* `record_video_dir`: Directory to save video recordings as `.mp4` files
* `record_video_size` (default: `ViewportSize`): The frame size (width, height) of the video recording.
* `record_video_framerate` (default: `30`): The framerate to use for the video recording.
* `record_har_path`: Path to save network trace files as `.har` format
* `traces_dir`: Directory to save complete trace files for debugging
* `record_har_content` (default: `'embed'`): HAR content mode (`'omit'`, `'embed'`, `'attach'`)
* `record_har_mode` (default: `'full'`): HAR recording mode (`'full'`, `'minimal'`)

### Advanced Options

* `disable_security` (default: `False`): ⚠️ **NOT RECOMMENDED** - Disables all browser security features
* `deterministic_rendering` (default: `False`): ⚠️ **NOT RECOMMENDED** - Forces consistent rendering but reduces performance

***

### Browser vs BrowserSession
`Browser` is an alias for `BrowserSession` - they are exactly the same class:
Use `Browser` for cleaner, more intuitive code.


## Real Browser
Connect your existing Chrome browser to preserve authentication.

### Basic Example

```python  theme={null}
from browser_use import Agent, Browser, ChatOpenAI

# Connect to your existing Chrome browser
browser = Browser(
    executable_path='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    user_data_dir='~/Library/Application Support/Google/Chrome',
    profile_directory='Default',
)

agent = Agent(
    task='Visit https://duckduckgo.com and search for "browser-use founders"',
    browser=browser,
    llm=ChatOpenAI(model='gpt-4.1-mini'),
)
async def main():
	await agent.run()
```

> **Note:** You need to fully close chrome before running this example. Also, Google blocks this approach currently so we use DuckDuckGo instead.

### How it Works

1. **`executable_path`** - Path to your Chrome installation
2. **`user_data_dir`** - Your Chrome profile folder (keeps cookies, extensions, bookmarks)
3. **`profile_directory`** - Specific profile name (Default, Profile 1, etc.)

### Platform Paths

```python  theme={null}
# macOS
executable_path='/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
user_data_dir='~/Library/Application Support/Google/Chrome'

# Windows
executable_path='C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe'
user_data_dir='%LOCALAPPDATA%\\Google\\Chrome\\User Data'

# Linux
executable_path='/usr/bin/google-chrome'
user_data_dir='~/.config/google-chrome'
```

## Remote Browser
#### Browser-Use Cloud Browser or CDP URL

The easiest way to use a cloud browser is with the built-in Browser-Use cloud service:

```python  theme={null}
from browser_use import Agent, Browser, ChatBrowserUse

# Simple: Use Browser-Use cloud browser service
browser = Browser(
    use_cloud=True,  # Automatically provisions a cloud browser
)

# Advanced: Configure cloud browser parameters
# Using this settings can bypass any captcha protection on any website
browser = Browser(
    cloud_profile_id='your-profile-id',  # Optional: specific browser profile
    cloud_proxy_country_code='us',  # Optional: proxy location (us, uk, fr, it, jp, au, de, fi, ca, in)
    cloud_timeout=30,  # Optional: session timeout in minutes (MAX free: 15min, paid: 240min)
)

# Or use a CDP URL from any cloud browser provider
browser = Browser(
    cdp_url="http://remote-server:9222"  # Get a CDP URL from any provider
)

agent = Agent(
    task="Your task here",
    llm=ChatBrowserUse(),
    browser=browser,
)
```

**Prerequisites:**

1. Get an API key from [cloud.browser-use.com](https://cloud.browser-use.com/new-api-key)
2. Set BROWSER\_USE\_API\_KEY environment variable

**Cloud Browser Parameters:**

* `cloud_profile_id`: UUID of a browser profile (optional, uses default if not specified)
* `cloud_proxy_country_code`: Country code for proxy location - supports: us, uk, fr, it, jp, au, de, fi, ca, in
* `cloud_timeout`: Session timeout in minutes (free users: max 15 min, paid users: max 240 min)

**Benefits:**

* ✅ No local browser setup required
* ✅ Scalable and fast cloud infrastructure
* ✅ Automatic provisioning and teardown
* ✅ Built-in authentication handling
* ✅ Optimized for browser automation
* ✅ Global proxy support for geo-restricted content

#### Proxy Connection
```python  theme={null}

from browser_use import Agent, Browser, ChatBrowserUse
from browser_use.browser import ProxySettings

browser = Browser(
    headless=False,
    proxy=ProxySettings(
        server="http://proxy-server:8080",
        username="proxy-user",
        password="proxy-pass"
    ),
    cdp_url="http://remote-server:9222"
)


agent = Agent(
    task="Your task here",
    llm=ChatBrowserUse(),
    browser=browser,
)
```
