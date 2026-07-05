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
/// shape (`{"actions": [...] , "memory": ...}`) and a single legacy action object
/// (`{"action": "click", "index": 0}`).
pub(crate) fn parse_output(response: &str) -> anyhow::Result<AgentOutput> {
    let stripped = strip_code_fence(response.trim());

    if let Ok(output) = serde_json::from_str::<AgentOutput>(stripped) {
        if !output.actions.is_empty() {
            return Ok(output);
        }
    }

    let action: AgentAction = serde_json::from_str(stripped)
        .map_err(|error| anyhow::anyhow!("could not parse agent output: {error}"))?;
    Ok(AgentOutput {
        actions: vec![action],
        ..Default::default()
    })
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
