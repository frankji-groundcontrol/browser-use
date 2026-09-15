//! Command-line argument parsing.
//!
//! Parsing is a pure function over the argument vector, separate from execution,
//! so every flag combination is testable without spawning a process, a browser,
//! or a network call. `main` stays a thin dispatch over [`Command`].

use anyhow::{anyhow, bail, Result};

/// Exit status for a usage error, matching the common convention (`2` = misuse).
pub const EXIT_USAGE: i32 = 2;

pub const USAGE: &str = "\
browser-use-rs — drive a real browser with an LLM

USAGE:
    browser-use-rs <TASK>            run the autonomous agent on TASK
    browser-use-rs --mcp             run as an MCP server over stdio
    browser-use-rs --help|--version

OPTIONS:
    -n, --max-steps <N>   step budget for the agent (default: 10)
        --vision          attach a screenshot to each model prompt
        --model <NAME>    override BROWSER_USE_LLM_MODEL for this run
        --headful         show the browser window instead of running headless

CONFIGURATION:
    Settings resolve from the process environment, then
    ~/.config/browser-use/.env (override with BROWSER_USE_ENV_FILE), then the
    macOS Keychain for the API key only.

        BROWSER_USE_LLM_BASE_URL   endpoint base, used exactly as given
        BROWSER_USE_LLM_API_KEY    credential
        BROWSER_USE_LLM_API        openai-responses | openai-chat | anthropic-messages
        BROWSER_USE_LLM_MODEL      model id

EXAMPLES:
    browser-use-rs \"find the top story on news.ycombinator.com\"
    browser-use-rs --headful -n 20 --vision \"log into the dashboard\"
";

/// Default agent step budget. Low on purpose: a runaway loop costs tokens.
pub const DEFAULT_MAX_STEPS: usize = 10;

/// What the process was asked to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Serve MCP over stdio.
    Mcp,
    /// Print usage.
    Help,
    /// Print the crate version.
    Version,
    /// Run the autonomous agent on a task.
    Run(RunOptions),
}

/// Options for a `Run` command.
#[derive(Debug, Clone, PartialEq)]
pub struct RunOptions {
    pub task: String,
    pub max_steps: usize,
    pub use_vision: bool,
    pub model: Option<String>,
    pub headful: bool,
}

/// Parses arguments **excluding** the program name.
///
/// `--mcp` is checked first and wins outright: it is how every agent launches
/// this binary, so no later parsing decision may affect it.
pub fn parse(args: &[String]) -> Result<Command> {
    if args.iter().any(|arg| arg == "--mcp") {
        return Ok(Command::Mcp);
    }
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ok(Command::Help);
    }
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        return Ok(Command::Version);
    }
    if args.is_empty() {
        bail!("no task given");
    }

    let mut task: Option<String> = None;
    let mut max_steps = DEFAULT_MAX_STEPS;
    let mut use_vision = false;
    let mut model = None;
    let mut headful = false;
    let mut rest_is_task = false;

    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        // Everything after `--` is the task, even if it starts with a dash.
        if !rest_is_task && arg == "--" {
            rest_is_task = true;
            index += 1;
            continue;
        }
        if !rest_is_task && arg.starts_with('-') {
            match arg {
                "-n" | "--max-steps" => {
                    let raw = args
                        .get(index + 1)
                        .ok_or_else(|| anyhow!("{arg} needs a number"))?;
                    max_steps = raw
                        .parse::<usize>()
                        .map_err(|_| anyhow!("{arg} expects a number, got {raw:?}"))?;
                    if max_steps == 0 {
                        bail!("{arg} must be at least 1");
                    }
                    index += 2;
                }
                "--model" => {
                    model = Some(
                        args.get(index + 1)
                            .ok_or_else(|| anyhow!("--model needs a name"))?
                            .clone(),
                    );
                    index += 2;
                }
                "--vision" => {
                    use_vision = true;
                    index += 1;
                }
                "--headful" => {
                    headful = true;
                    index += 1;
                }
                other => bail!("unknown option {other:?}"),
            }
            continue;
        }
        // A positional argument is the task; a second one is a mistake worth
        // reporting rather than silently joining or ignoring.
        if task.is_some() {
            bail!("unexpected extra argument {arg:?}; quote the task as one argument");
        }
        task = Some(args[index].clone());
        index += 1;
    }

    let task = task.ok_or_else(|| anyhow!("no task given"))?;
    if task.trim().is_empty() {
        bail!("the task is empty");
    }
    Ok(Command::Run(RunOptions {
        task,
        max_steps,
        use_vision,
        model,
        headful,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    fn run_of(items: &[&str]) -> RunOptions {
        match parse(&args(items)).unwrap() {
            Command::Run(options) => options,
            other => panic!("expected Run, got {other:?}"),
        }
    }

    #[test]
    fn mcp_wins_over_everything_else() {
        // Every agent launches with --mcp; no other argument may change that.
        assert_eq!(parse(&args(&["--mcp"])).unwrap(), Command::Mcp);
        assert_eq!(
            parse(&args(&["--mcp", "--vision", "some task"])).unwrap(),
            Command::Mcp
        );
        assert_eq!(parse(&args(&["a task", "--mcp"])).unwrap(), Command::Mcp);
    }

    #[test]
    fn a_bare_task_runs_with_defaults() {
        let options = run_of(&["find the cheapest flight"]);
        assert_eq!(options.task, "find the cheapest flight");
        assert_eq!(options.max_steps, DEFAULT_MAX_STEPS);
        assert!(!options.use_vision);
        assert!(!options.headful);
        assert_eq!(options.model, None);
    }

    #[test]
    fn flags_parse_in_any_order() {
        let before = run_of(&["--vision", "-n", "3", "--headful", "task"]);
        let after = run_of(&["task", "--headful", "--vision", "-n", "3"]);
        assert_eq!(before, after);
        assert_eq!(before.max_steps, 3);
        assert!(before.use_vision && before.headful);
    }

    #[test]
    fn model_override_is_captured() {
        assert_eq!(
            run_of(&["--model", "bu-2-0", "t"]).model.as_deref(),
            Some("bu-2-0")
        );
    }

    #[test]
    fn help_and_version_are_their_own_commands() {
        for flag in ["--help", "-h"] {
            assert_eq!(parse(&args(&[flag])).unwrap(), Command::Help);
        }
        for flag in ["--version", "-V"] {
            assert_eq!(parse(&args(&[flag])).unwrap(), Command::Version);
        }
        // ...and beat a task, so `browser-use-rs "x" --help` explains itself.
        assert_eq!(parse(&args(&["a task", "--help"])).unwrap(), Command::Help);
    }

    #[test]
    fn no_arguments_is_an_error_not_a_silent_success() {
        // The whole point: an empty invocation must not look like it worked.
        assert!(parse(&args(&[])).is_err());
    }

    #[test]
    fn a_typo_in_the_mcp_flag_is_rejected_loudly() {
        // The costly real-world case: a mistyped MCP config used to start a
        // process that did nothing and exited 0.
        let error = parse(&args(&["--mpc"])).unwrap_err().to_string();
        assert!(error.contains("--mpc"), "should quote the typo: {error}");
    }

    #[test]
    fn max_steps_rejects_nonsense() {
        assert!(parse(&args(&["-n", "abc", "t"])).is_err());
        assert!(
            parse(&args(&["-n", "0", "t"])).is_err(),
            "0 steps cannot run"
        );
        assert!(parse(&args(&["-n"])).is_err(), "missing value");
    }

    #[test]
    fn a_second_positional_is_reported_rather_than_guessed() {
        let error = parse(&args(&["do this", "and that"]))
            .unwrap_err()
            .to_string();
        assert!(error.contains("quote the task"), "got: {error}");
    }

    #[test]
    fn double_dash_lets_a_task_start_with_a_dash() {
        assert_eq!(
            run_of(&["--", "--weird looking task"]).task,
            "--weird looking task"
        );
    }

    #[test]
    fn an_empty_task_is_rejected() {
        assert!(parse(&args(&["   "])).is_err());
    }
}
