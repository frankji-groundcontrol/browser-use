//! LLM configuration: one explicit environment surface, one wire format.
//!
//! Every setting is a `BROWSER_USE_LLM_*` variable the operator sets directly.
//! Credential presence never selects a backend — [`LlmApi`] does, and only it.
//! That is the whole point of this module: exporting `OPENAI_API_KEY` for some
//! unrelated tool must not silently change which model the agent talks to.

use std::{collections::HashMap, env, path::PathBuf, process::Command, sync::OnceLock};

use anyhow::{anyhow, Context, Result};

/// Keychain service name holding the LLM API key.
pub const KEYCHAIN_SERVICE: &str = "browser-use-llm";
/// Only this setting is ever read from the Keychain — it stores a secret, not a
/// configuration file.
const SECRET_VAR: &str = "BROWSER_USE_LLM_API_KEY";

/// Path of the `.env` file: `$BROWSER_USE_ENV_FILE`, else
/// `~/.config/browser-use/.env`.
pub fn dotenv_path() -> Option<PathBuf> {
    if let Some(explicit) = env::var_os("BROWSER_USE_ENV_FILE") {
        let path = PathBuf::from(explicit);
        return (!path.as_os_str().is_empty()).then_some(path);
    }
    env::var_os("HOME").map(|home| {
        PathBuf::from(home)
            .join(".config")
            .join("browser-use")
            .join(".env")
    })
}

/// Parses `KEY=value` lines. Tolerates `export ` prefixes, surrounding quotes,
/// `#` comments, and whitespace around `=`. Empty values are dropped so a blank
/// line in the file cannot mask a real value from a lower layer.
fn parse_dotenv(text: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in text.lines() {
        let line = line.trim().strip_prefix("export ").unwrap_or(line.trim());
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, raw)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || key.contains(char::is_whitespace) {
            continue;
        }
        let raw = raw.trim();
        // A quoted value runs to its *matching* closing quote, so anything after
        // it (typically a trailing comment) is discarded and a `#` inside the
        // quotes is preserved. Only an unquoted value is cut at a comment marker.
        let value = match raw.chars().next() {
            Some(quote @ ('"' | '\'')) => match raw[1..].find(quote) {
                Some(end) => raw[1..=end].to_owned(),
                // Unterminated quote: take the rest verbatim rather than losing it.
                None => raw[1..].to_owned(),
            },
            _ => raw
                .split_once(" #")
                .map_or(raw, |(before, _)| before)
                .trim()
                .to_owned(),
        };
        if !value.is_empty() {
            out.insert(key.to_owned(), value);
        }
    }
    out
}

fn dotenv_values() -> &'static HashMap<String, String> {
    static CACHE: OnceLock<HashMap<String, String>> = OnceLock::new();
    CACHE.get_or_init(|| {
        dotenv_path()
            .and_then(|path| std::fs::read_to_string(path).ok())
            .map(|text| parse_dotenv(&text))
            .unwrap_or_default()
    })
}

/// Reads the API key from the macOS Keychain. Any failure (not macOS, no entry,
/// user denied access) is simply "not found" — this is a fallback layer, not a
/// requirement.
fn keychain_secret(_key: &str) -> Option<String> {
    static CACHE: OnceLock<Option<String>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let output = Command::new("security")
                .args(["find-generic-password", "-s", KEYCHAIN_SERVICE, "-w"])
                .output()
                .ok()?;
            if !output.status.success() {
                return None;
            }
            let value = String::from_utf8(output.stdout).ok()?.trim().to_owned();
            (!value.is_empty()).then_some(value)
        })
        .clone()
}

/// Composes the three lookup layers, most explicit first: process environment,
/// then the `.env` file, then the Keychain.
///
/// The Keychain layer is consulted for [`SECRET_VAR`] **only** — it stores a
/// password, not a configuration file, and that restriction is enforced here
/// rather than inside the Keychain reader so the composition itself is what
/// gets tested.
///
/// Taking each layer as an argument keeps precedence testable without touching
/// process globals, the real filesystem, or the user's Keychain.
fn layered<'a>(
    from_env: impl Fn(&str) -> Option<String> + 'a,
    from_file: &'a HashMap<String, String>,
    from_keychain: impl Fn(&str) -> Option<String> + 'a,
) -> impl Fn(&str) -> Option<String> + 'a {
    move |key| {
        from_env(key)
            .or_else(|| from_file.get(key).cloned())
            .or_else(|| (key == SECRET_VAR).then(|| from_keychain(key)).flatten())
    }
}

/// Default sampling temperature (Python's `ChatOpenAI` default for the agent).
pub const DEFAULT_TEMPERATURE: f32 = 0.7;
/// Default output cap. Anthropic Messages requires one; the OpenAI formats do
/// not send it.
pub const DEFAULT_MAX_TOKENS: u32 = 4096;

/// Which wire protocol to speak. This is the only backend selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LlmApi {
    /// `POST {base}/responses` — OpenAI's current API.
    #[default]
    OpenAiResponses,
    /// `POST {base}/chat/completions` — the older OpenAI route, and what most
    /// OpenAI-compatible gateways implement.
    OpenAiChat,
    /// `POST {base}/messages` — Anthropic's native Messages API.
    AnthropicMessages,
    /// AWS Bedrock Converse API (credentials come from the AWS chain, so
    /// `BROWSER_USE_LLM_BASE_URL` and `_API_KEY` are not used).
    #[cfg(feature = "bedrock")]
    Bedrock,
}

impl LlmApi {
    /// Parses `BROWSER_USE_LLM_API`. Hyphens, underscores, and case are all
    /// accepted; an unknown value is an error rather than a silent default, so a
    /// typo cannot quietly change which protocol is spoken.
    pub fn parse(value: &str) -> Result<Self> {
        let normalized = value.trim().to_ascii_lowercase().replace(['-', ' '], "_");
        match normalized.as_str() {
            "openai_responses" | "responses" => Ok(Self::OpenAiResponses),
            "openai_chat" | "openai_chat_completions" | "chat_completions" | "chat" => {
                Ok(Self::OpenAiChat)
            }
            "anthropic_messages" | "anthropic" | "messages" => Ok(Self::AnthropicMessages),
            #[cfg(feature = "bedrock")]
            "bedrock" => Ok(Self::Bedrock),
            #[cfg(not(feature = "bedrock"))]
            "bedrock" => Err(anyhow!(
                "BROWSER_USE_LLM_API=bedrock requires a build with --features bedrock"
            )),
            _ => Err(anyhow!(
                "invalid BROWSER_USE_LLM_API={value:?}; expected one of: openai-responses, openai-chat, anthropic-messages{}",
                if cfg!(feature = "bedrock") { ", bedrock" } else { "" }
            )),
        }
    }

    /// Path appended to the base URL for this protocol.
    pub fn path(self) -> &'static str {
        match self {
            Self::OpenAiResponses => "responses",
            Self::OpenAiChat => "chat/completions",
            Self::AnthropicMessages => "messages",
            #[cfg(feature = "bedrock")]
            Self::Bedrock => "",
        }
    }

    /// Whether this protocol is driven over plain HTTP by [`crate::LlmClient`].
    /// Bedrock goes through the AWS SDK instead.
    pub fn is_http(self) -> bool {
        #[cfg(feature = "bedrock")]
        if matches!(self, Self::Bedrock) {
            return false;
        }
        true
    }
}

/// Resolved LLM settings.
#[derive(Debug, Clone, PartialEq)]
pub struct LlmConfig {
    /// Credential. Sent as `Authorization: Bearer` (OpenAI) or `x-api-key`
    /// (Anthropic).
    pub api_key: String,
    /// Base URL, used **verbatim** for the first attempt.
    pub base_url: String,
    /// Model id.
    pub model: String,
    /// Wire protocol.
    pub api: LlmApi,
    /// Optional sampling temperature.
    pub temperature: Option<f32>,
    /// Output cap. Required by Anthropic Messages; unused by the OpenAI formats.
    pub max_tokens: u32,
}

impl LlmConfig {
    /// Loads configuration from the process environment.
    pub fn from_env() -> Result<Self> {
        Self::from_env_with_model_override(None)
    }

    /// Loads configuration, letting an MCP tool argument override the model.
    ///
    /// Each setting is resolved from the process environment first, then the
    /// `.env` file, then (for the API key only) the macOS Keychain. So an agent
    /// config can still pin a value explicitly, while the common case is a
    /// single `.env` and no secrets in any agent's config file at all.
    pub fn from_env_with_model_override(model_override: Option<String>) -> Result<Self> {
        let lookup = layered(
            |key| env::var(key).ok(),
            dotenv_values(),
            keychain_secret,
        );
        Self::resolve(lookup, model_override)
    }

    /// Split from the environment read so precedence is testable without
    /// mutating process-global state.
    pub(crate) fn resolve(
        lookup: impl Fn(&str) -> Option<String>,
        model_override: Option<String>,
    ) -> Result<Self> {
        let read = |key: &str| {
            lookup(key)
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        };

        let api = match read("BROWSER_USE_LLM_API") {
            Some(value) => LlmApi::parse(&value)?,
            None => LlmApi::default(),
        };

        // Bedrock authenticates through the AWS credential chain, so it needs
        // neither of these; demanding them would be a pointless barrier.
        let needs_http_credentials = api.is_http();

        let api_key = match read("BROWSER_USE_LLM_API_KEY") {
            Some(key) => key,
            None if needs_http_credentials => {
                return Err(anyhow!(
                    "no LLM credentials: set BROWSER_USE_LLM_API_KEY"
                ))
            }
            None => String::new(),
        };

        let base_url = match read("BROWSER_USE_LLM_BASE_URL") {
            Some(base) => base.trim_end_matches('/').to_owned(),
            None if needs_http_credentials => {
                return Err(anyhow!(
                    "no LLM endpoint: set BROWSER_USE_LLM_BASE_URL (e.g. https://api.openai.com/v1, https://api.anthropic.com/v1, or your gateway)"
                ))
            }
            None => String::new(),
        };

        let model = model_override
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .or_else(|| read("BROWSER_USE_LLM_MODEL"))
            .ok_or_else(|| anyhow!("no LLM model: set BROWSER_USE_LLM_MODEL"))?;

        let temperature = match read("BROWSER_USE_LLM_TEMPERATURE") {
            Some(value) => Some(
                value
                    .parse::<f32>()
                    .with_context(|| format!("invalid BROWSER_USE_LLM_TEMPERATURE={value:?}"))?,
            ),
            None => Some(DEFAULT_TEMPERATURE),
        };

        let max_tokens = match read("BROWSER_USE_LLM_MAX_TOKENS") {
            Some(value) => value
                .parse::<u32>()
                .with_context(|| format!("invalid BROWSER_USE_LLM_MAX_TOKENS={value:?}"))?,
            None => DEFAULT_MAX_TOKENS,
        };

        Ok(Self {
            api_key,
            base_url,
            model,
            api,
            temperature,
            max_tokens,
        })
    }

    /// The URL for the first attempt: the configured base, used as given.
    pub fn endpoint_url(&self) -> String {
        join(&self.base_url, self.api.path())
    }

    /// The one remaining candidate when the exact URL has no route there: a bare
    /// base gains `/v1`, a versioned one loses it. Gateways serve these routes at
    /// one root or the other, and which one is not discoverable in advance.
    pub fn fallback_url(&self) -> Option<String> {
        let alternate = alternate_api_root(&self.base_url);
        (alternate != self.base_url).then(|| join(&alternate, self.api.path()))
    }
}

fn join(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    if path.is_empty() {
        base.to_owned()
    } else {
        format!("{base}/{path}")
    }
}

/// The other plausible API root for `base`: a versioned base loses its version
/// segment, a bare one gains `/v1`.
pub(crate) fn alternate_api_root(base: &str) -> String {
    let trimmed = base.trim_end_matches('/');
    match trimmed.rsplit_once('/') {
        Some((prefix, last)) if is_version_segment(last) => prefix.to_owned(),
        _ => format!("{trimmed}/v1"),
    }
}

/// Whether a path segment is an API version: `v` followed by at least one digit,
/// optionally continued with letters/digits/dots (`v1beta`, `v2`, `v1.0`),
/// case-insensitive. Lookalikes without leading digits ("version", "vpn") are
/// NOT versions.
pub(crate) fn is_version_segment(segment: &str) -> bool {
    let lowered = segment.to_ascii_lowercase();
    let Some(rest) = lowered.strip_prefix('v') else {
        return false;
    };
    let digits = rest
        .bytes()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    digits > 0
        && rest[digits..]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |key| {
            pairs
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    #[test]
    fn api_parses_every_documented_spelling() {
        for value in ["openai-responses", "responses", "OpenAI_Responses"] {
            assert_eq!(LlmApi::parse(value).unwrap(), LlmApi::OpenAiResponses);
        }
        for value in ["openai-chat", "chat_completions", "chat"] {
            assert_eq!(LlmApi::parse(value).unwrap(), LlmApi::OpenAiChat);
        }
        for value in ["anthropic-messages", "anthropic", "messages"] {
            assert_eq!(LlmApi::parse(value).unwrap(), LlmApi::AnthropicMessages);
        }
    }

    #[test]
    fn unknown_api_errors_and_lists_the_valid_values() {
        let error = LlmApi::parse("gpt").unwrap_err().to_string();
        assert!(error.contains("gpt"), "should quote the input: {error}");
        assert!(
            error.contains("anthropic-messages"),
            "should list valid values: {error}"
        );
    }

    #[test]
    fn each_api_has_its_own_route() {
        assert_eq!(LlmApi::OpenAiResponses.path(), "responses");
        assert_eq!(LlmApi::OpenAiChat.path(), "chat/completions");
        assert_eq!(LlmApi::AnthropicMessages.path(), "messages");
    }

    #[test]
    fn base_url_is_used_exactly_as_given() {
        let config = LlmConfig::resolve(
            env_of(&[
                ("BROWSER_USE_LLM_API_KEY", "k"),
                ("BROWSER_USE_LLM_BASE_URL", "https://gw.example/v1"),
                ("BROWSER_USE_LLM_MODEL", "m"),
                ("BROWSER_USE_LLM_API", "anthropic-messages"),
            ]),
            None,
        )
        .unwrap();
        assert_eq!(config.endpoint_url(), "https://gw.example/v1/messages");
    }

    #[test]
    fn bare_host_falls_back_to_v1_but_only_second() {
        let config = LlmConfig::resolve(
            env_of(&[
                ("BROWSER_USE_LLM_API_KEY", "k"),
                ("BROWSER_USE_LLM_BASE_URL", "https://gw.example"),
                ("BROWSER_USE_LLM_MODEL", "m"),
                ("BROWSER_USE_LLM_API", "openai-chat"),
            ]),
            None,
        )
        .unwrap();
        assert_eq!(
            config.endpoint_url(),
            "https://gw.example/chat/completions",
            "exact URL is tried first"
        );
        assert_eq!(
            config.fallback_url().unwrap(),
            "https://gw.example/v1/chat/completions",
            "the /v1 root is the fallback, not the primary"
        );
    }

    #[test]
    fn versioned_host_falls_back_to_the_bare_root() {
        let config = LlmConfig::resolve(
            env_of(&[
                ("BROWSER_USE_LLM_API_KEY", "k"),
                ("BROWSER_USE_LLM_BASE_URL", "https://gw.example/v1"),
                ("BROWSER_USE_LLM_MODEL", "m"),
                ("BROWSER_USE_LLM_API", "openai-chat"),
            ]),
            None,
        )
        .unwrap();
        assert_eq!(config.endpoint_url(), "https://gw.example/v1/chat/completions");
        assert_eq!(
            config.fallback_url().unwrap(),
            "https://gw.example/chat/completions"
        );
    }

    #[test]
    fn removed_variables_are_ignored() {
        let error = LlmConfig::resolve(
            env_of(&[
                ("OPENAI_API_KEY", "legacy"),
                ("OPENAI_BASE_URL", "https://legacy.example/v1"),
                ("ANTHROPIC_AUTH_TOKEN", "legacy"),
                ("ANTHROPIC_BASE_URL", "https://legacy.example"),
                ("MODEL_PROVIDER", "bedrock"),
                ("BROWSER_USE_OPENAI_API", "chat"),
                ("BROWSER_USE_API_KEY", "legacy"),
            ]),
            None,
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("BROWSER_USE_LLM_API_KEY"),
            "legacy vars must not satisfy config: {error}"
        );
    }

    #[test]
    fn missing_settings_name_the_variable_to_set() {
        let base = [
            ("BROWSER_USE_LLM_API_KEY", "k"),
            ("BROWSER_USE_LLM_BASE_URL", "https://gw.example/v1"),
            ("BROWSER_USE_LLM_MODEL", "m"),
        ];
        for (missing, expected) in [
            ("BROWSER_USE_LLM_API_KEY", "BROWSER_USE_LLM_API_KEY"),
            ("BROWSER_USE_LLM_BASE_URL", "BROWSER_USE_LLM_BASE_URL"),
            ("BROWSER_USE_LLM_MODEL", "BROWSER_USE_LLM_MODEL"),
        ] {
            let kept: Vec<_> = base.iter().filter(|(k, _)| *k != missing).collect();
            let error = LlmConfig::resolve(
                move |key| {
                    kept.iter()
                        .find(|(name, _)| *name == key)
                        .map(|(_, value)| (*value).to_owned())
                },
                None,
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains(expected), "{missing} error was: {error}");
        }
    }

    #[test]
    fn model_override_beats_env() {
        let config = LlmConfig::resolve(
            env_of(&[
                ("BROWSER_USE_LLM_API_KEY", "k"),
                ("BROWSER_USE_LLM_BASE_URL", "https://gw.example/v1"),
                ("BROWSER_USE_LLM_MODEL", "from-env"),
            ]),
            Some("from-arg".to_owned()),
        )
        .unwrap();
        assert_eq!(config.model, "from-arg");
    }

    #[test]
    fn defaults_apply_for_temperature_and_max_tokens() {
        let config = LlmConfig::resolve(
            env_of(&[
                ("BROWSER_USE_LLM_API_KEY", "k"),
                ("BROWSER_USE_LLM_BASE_URL", "https://gw.example/v1"),
                ("BROWSER_USE_LLM_MODEL", "m"),
            ]),
            None,
        )
        .unwrap();
        assert_eq!(config.temperature, Some(DEFAULT_TEMPERATURE));
        assert_eq!(config.max_tokens, DEFAULT_MAX_TOKENS);
        assert_eq!(config.api, LlmApi::OpenAiResponses, "default protocol");
    }

    #[test]
    fn dotenv_parses_exports_quotes_and_comments() {
        let parsed = parse_dotenv(
            r#"
# a comment
BROWSER_USE_LLM_MODEL=gpt-5.6-sol
export BROWSER_USE_LLM_API_KEY="sk-quoted"
BROWSER_USE_LLM_BASE_URL='https://gw.example/v1'   # trailing comment
EMPTY=
BROWSER_USE_LLM_API = openai-chat
not a pair
"#,
        );
        assert_eq!(parsed.get("BROWSER_USE_LLM_MODEL").unwrap(), "gpt-5.6-sol");
        assert_eq!(
            parsed.get("BROWSER_USE_LLM_API_KEY").unwrap(),
            "sk-quoted",
            "quotes are stripped"
        );
        assert_eq!(
            parsed.get("BROWSER_USE_LLM_BASE_URL").unwrap(),
            "https://gw.example/v1",
            "an unquoted trailing comment is not part of the value"
        );
        assert_eq!(
            parsed.get("BROWSER_USE_LLM_API").unwrap(),
            "openai-chat",
            "whitespace around = is tolerated"
        );
        assert!(parsed.get("EMPTY").is_none(), "empty values are dropped");
        assert!(parsed.get("not a pair").is_none());
    }

    #[test]
    fn a_hash_inside_quotes_is_kept() {
        let parsed = parse_dotenv(r#"BROWSER_USE_LLM_API_KEY="sk-with#hash""#);
        assert_eq!(parsed.get("BROWSER_USE_LLM_API_KEY").unwrap(), "sk-with#hash");
    }

    #[test]
    fn process_env_wins_over_the_dotenv_file() {
        let file = parse_dotenv("BROWSER_USE_LLM_MODEL=from-file");
        let resolved = layered(
            |k| (k == "BROWSER_USE_LLM_MODEL").then(|| "from-env".to_owned()),
            &file,
            |_| None,
        );
        assert_eq!(resolved("BROWSER_USE_LLM_MODEL").unwrap(), "from-env");
    }

    #[test]
    fn the_dotenv_file_fills_gaps_the_environment_leaves() {
        let file = parse_dotenv("BROWSER_USE_LLM_MODEL=from-file");
        let resolved = layered(|_| None, &file, |_| None);
        assert_eq!(resolved("BROWSER_USE_LLM_MODEL").unwrap(), "from-file");
    }

    #[test]
    fn the_keychain_is_the_last_resort_and_only_for_the_key() {
        let file = parse_dotenv("");
        let resolved = layered(|_| None, &file, |_| Some("sk-from-keychain".to_owned()));
        assert_eq!(
            resolved("BROWSER_USE_LLM_API_KEY").unwrap(),
            "sk-from-keychain"
        );
        assert!(
            resolved("BROWSER_USE_LLM_MODEL").is_none(),
            "the keychain holds a secret, not the whole configuration"
        );
    }

    #[test]
    fn the_dotenv_file_beats_the_keychain() {
        let file = parse_dotenv("BROWSER_USE_LLM_API_KEY=sk-from-file");
        let resolved = layered(|_| None, &file, |_| Some("sk-from-keychain".to_owned()));
        assert_eq!(resolved("BROWSER_USE_LLM_API_KEY").unwrap(), "sk-from-file");
    }

    #[test]
    fn version_segment_recognizes_shapes_and_rejects_lookalikes() {
        for segment in ["v1", "V1", "v2", "v1beta", "v1.0"] {
            assert!(is_version_segment(segment), "{segment} is a version");
        }
        for segment in ["version", "vpn", "view", "v"] {
            assert!(!is_version_segment(segment), "{segment} is not a version");
        }
    }
}
