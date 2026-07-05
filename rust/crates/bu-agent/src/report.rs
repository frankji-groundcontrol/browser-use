//! Agent run summary + Python-compatible report formatting.

/// Summary returned by an autonomous agent run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AgentRunReport {
    /// Number of model-directed steps attempted.
    pub steps: usize,
    /// Whether the model marked the task as successful.
    pub success: bool,
    /// Final model-provided task result, if any.
    pub final_result: String,
    /// Per-step errors encountered during the run.
    pub errors: Vec<String>,
    /// URLs observed while the agent was running.
    pub urls_visited: Vec<String>,
}

impl AgentRunReport {
    /// Formats this report with the Python MCP retry tool wording: a capitalized
    /// success bool, and Final result / Errors / URLs sections included only when
    /// non-empty (see `_retry_with_browser_use_agent`).
    pub fn to_python_report(&self) -> String {
        let mut lines = vec![
            format!("Task completed in {} steps", self.steps),
            format!("Success: {}", if self.success { "True" } else { "False" }),
        ];

        if !self.final_result.trim().is_empty() {
            lines.push(format!("\nFinal result:\n{}", self.final_result));
        }
        if !self.errors.is_empty() {
            let errors =
                serde_json::to_string_pretty(&self.errors).unwrap_or_else(|_| "[]".to_owned());
            lines.push(format!("\nErrors encountered:\n{errors}"));
        }
        let urls: Vec<&str> = self
            .urls_visited
            .iter()
            .map(String::as_str)
            .filter(|url| !url.is_empty())
            .collect();
        if !urls.is_empty() {
            lines.push(format!("\nURLs visited: {}", urls.join(", ")));
        }

        lines.join("\n")
    }
}

/// Appends `url` to `urls` if it is non-empty and not already present.
pub(crate) fn push_unique_url(urls: &mut Vec<String>, url: String) {
    if url.is_empty() || urls.iter().any(|seen| seen == &url) {
        return;
    }
    urls.push(url);
}
