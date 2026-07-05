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
    /// Formats this report with the Python MCP retry tool wording.
    pub fn to_python_report(&self) -> String {
        format!(
            "Task completed in {steps} steps\nSuccess: {success}\nFinal result: {result}\nErrors encountered: {errors}\nURLs visited: {urls}",
            steps = self.steps,
            success = self.success,
            result = self.final_result,
            errors = serde_json::to_string(&self.errors).unwrap_or_else(|_| "[]".to_owned()),
            urls = self.urls_visited.join(",")
        )
    }
}

/// Appends `url` to `urls` if it is non-empty and not already present.
pub(crate) fn push_unique_url(urls: &mut Vec<String>, url: String) {
    if url.is_empty() || urls.iter().any(|seen| seen == &url) {
        return;
    }
    urls.push(url);
}
