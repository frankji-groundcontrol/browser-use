//! Agent action schema (multi-action output + reasoning) and lenient parsing.

use serde::Deserialize;

/// One browser action the model can request.
#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub(crate) enum AgentAction {
    Navigate {
        url: String,
    },
    Click {
        index: usize,
    },
    Type {
        index: usize,
        text: String,
    },
    Scroll {
        direction: ScrollDirection,
    },
    Extract {
        query: String,
    },
    Done {
        success: bool,
        #[serde(default)]
        result: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ScrollDirection {
    Down,
    Up,
}

impl ScrollDirection {
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            Self::Down => "down",
            Self::Up => "up",
        }
    }
}

/// A full model turn: reasoning fields plus an ordered list of actions to run.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct AgentOutput {
    #[serde(default)]
    pub evaluation_previous_goal: String,
    #[serde(default)]
    pub memory: String,
    #[serde(default)]
    pub next_goal: String,
    #[serde(default)]
    pub actions: Vec<AgentAction>,
}

/// Parses a model response into an [`AgentOutput`], accepting both the multi-action
/// shape (`{"actions": [...], "memory": ...}`) and a single legacy action object
/// (`{"action": "click", "index": 0}`).
///
/// A reasoning-only turn (`{"memory": ..., "actions": []}`) is returned with an
/// empty `actions` list (a no-op step) rather than treated as a parse error — it
/// is distinguished from a legacy single action by the presence of a string
/// `"action"` key.
pub(crate) fn parse_output(response: &str) -> anyhow::Result<AgentOutput> {
    let stripped = strip_code_fence(response.trim());
    let value: serde_json::Value = serde_json::from_str(stripped)
        .map_err(|error| anyhow::anyhow!("could not parse agent output as JSON: {error}"))?;

    // Legacy single-action object: {"action": "click", ...}.
    if value
        .get("action")
        .and_then(serde_json::Value::as_str)
        .is_some()
    {
        let action: AgentAction = serde_json::from_value(value)
            .map_err(|error| anyhow::anyhow!("could not parse agent action: {error}"))?;
        return Ok(AgentOutput {
            actions: vec![action],
            ..Default::default()
        });
    }

    // Multi-action / reasoning object (possibly with an empty actions list).
    serde_json::from_value(value)
        .map_err(|error| anyhow::anyhow!("could not parse agent output: {error}"))
}

fn strip_code_fence(text: &str) -> &str {
    let Some(after_opening) = text.strip_prefix("```") else {
        return text;
    };
    let after_language = after_opening
        .strip_prefix("json")
        .or_else(|| after_opening.strip_prefix("JSON"))
        .unwrap_or(after_opening)
        .trim_start_matches(['\r', '\n']);
    after_language
        .strip_suffix("```")
        .map(str::trim)
        .unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use super::parse_output;

    #[test]
    fn parses_legacy_single_action() {
        let out = parse_output(r#"{"action":"click","index":3}"#).unwrap();
        assert_eq!(out.actions.len(), 1);
    }

    #[test]
    fn parses_multi_action_with_reasoning() {
        let out = parse_output(
            r#"{"memory":"m","next_goal":"g","actions":[{"action":"type","index":0,"text":"hi"},{"action":"click","index":1}]}"#,
        )
        .unwrap();
        assert_eq!(out.actions.len(), 2);
        assert_eq!(out.memory, "m");
    }

    #[test]
    fn reasoning_only_turn_is_empty_noop_not_error() {
        let out = parse_output(
            r#"{"evaluation_previous_goal":"ok","memory":"noted","next_goal":"scroll","actions":[]}"#,
        )
        .unwrap();
        assert!(out.actions.is_empty());
        assert_eq!(out.memory, "noted");
    }

    #[test]
    fn strips_json_code_fence() {
        let out =
            parse_output("```json\n{\"action\":\"done\",\"success\":true,\"result\":\"x\"}\n```")
                .unwrap();
        assert_eq!(out.actions.len(), 1);
    }

    #[test]
    fn non_json_prose_is_an_error() {
        assert!(parse_output("I will click the button").is_err());
    }
}
