use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

pub const DEFAULT_PROMPT: &str =
    "Inspect git status and the current diff, then summarize the repository state and any risks.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveTestSurface {
    Doctor,
    CodexExec,
    CodexTuiApi,
    CodexTuiSubscription,
    ClaudeCodeApi,
    ClaudeCodeSubscription,
    ClaudeDesktopConfig,
    Report,
}

impl LiveTestSurface {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Doctor => "doctor",
            Self::CodexExec => "codex-exec",
            Self::CodexTuiApi => "codex-tui-api",
            Self::CodexTuiSubscription => "codex-tui-subscription",
            Self::ClaudeCodeApi => "claude-code-api",
            Self::ClaudeCodeSubscription => "claude-code-subscription",
            Self::ClaudeDesktopConfig => "claude-desktop-config",
            Self::Report => "report",
        }
    }

    pub fn all() -> &'static [Self] {
        &[
            Self::Doctor,
            Self::CodexExec,
            Self::CodexTuiApi,
            Self::CodexTuiSubscription,
            Self::ClaudeCodeApi,
            Self::ClaudeCodeSubscription,
            Self::ClaudeDesktopConfig,
            Self::Report,
        ]
    }
}

impl fmt::Display for LiveTestSurface {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for LiveTestSurface {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "doctor" => Ok(Self::Doctor),
            "codex-exec" => Ok(Self::CodexExec),
            "codex-tui-api" => Ok(Self::CodexTuiApi),
            "codex-tui-subscription" => Ok(Self::CodexTuiSubscription),
            "claude-code-api" => Ok(Self::ClaudeCodeApi),
            "claude-code-subscription" => Ok(Self::ClaudeCodeSubscription),
            "claude-desktop-config" => Ok(Self::ClaudeDesktopConfig),
            "report" => Ok(Self::Report),
            other => Err(format!("unknown live-test surface: {other}")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveTestRequest {
    pub surface: LiveTestSurface,
    pub repo: PathBuf,
    pub tokmeter_bin: String,
    pub prompt: String,
}

impl LiveTestRequest {
    pub fn new(surface: LiveTestSurface, repo: impl Into<PathBuf>) -> Self {
        Self {
            surface,
            repo: repo.into(),
            tokmeter_bin: "vc-tokmeter".to_owned(),
            prompt: DEFAULT_PROMPT.to_owned(),
        }
    }

    pub fn with_tokmeter_bin(mut self, tokmeter_bin: impl Into<String>) -> Self {
        self.tokmeter_bin = tokmeter_bin.into();
        self
    }

    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = prompt.into();
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveTestPlan {
    pub surface: LiveTestSurface,
    pub repo: PathBuf,
    pub event_log: PathBuf,
    pub report_dir: PathBuf,
    pub codex_jsonl: PathBuf,
    pub steps: Vec<LiveTestStep>,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveTestStep {
    pub title: String,
    pub command: Option<String>,
    pub output: Option<String>,
}

impl LiveTestStep {
    fn command(title: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            command: Some(command.into()),
            output: None,
        }
    }

    fn output(title: impl Into<String>, output: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            command: None,
            output: Some(output.into()),
        }
    }
}

pub fn plan_live_test(request: &LiveTestRequest) -> LiveTestPlan {
    let repo = request.repo.clone();
    let event_log = repo.join(".tokmeter").join("events.jsonl");
    let report_dir = repo.join(".tokmeter").join("report");
    let codex_jsonl = repo.join(".tokmeter").join("codex-exec.jsonl");
    let mut steps = Vec::new();
    let mut notes = Vec::new();

    match request.surface {
        LiveTestSurface::Doctor => {
            steps.push(LiveTestStep::command("Print resolved paths", "pwd"));
            steps.push(LiveTestStep::command("Check cargo", "command -v cargo"));
            steps.push(LiveTestStep::command("Check ripgrep", "command -v rg"));
            steps.push(LiveTestStep::command("Check Codex CLI", "command -v codex"));
            steps.push(LiveTestStep::command(
                "Check Claude Code CLI",
                "command -v claude",
            ));
            steps.push(LiveTestStep::command(
                "Check OpenAI API key",
                "test -n \"$OPENAI_API_KEY\" && echo OPENAI_API_KEY=set || echo OPENAI_API_KEY=unset",
            ));
            steps.push(LiveTestStep::command(
                "Check Anthropic API key",
                "test -n \"$ANTHROPIC_API_KEY\" && echo ANTHROPIC_API_KEY=set || echo ANTHROPIC_API_KEY=unset",
            ));
            notes.push(
                "Doctor renders prerequisites only; it does not start live sessions.".to_owned(),
            );
        }
        LiveTestSurface::CodexExec => {
            steps.push(init_step(request, &repo));
            steps.push(LiveTestStep::command(
                "Run Codex exec with structured JSON output",
                format!(
                    "cd {} && codex exec --json {} > {}",
                    shell_quote_path(&repo),
                    shell_quote(&request.prompt),
                    shell_quote_path(&codex_jsonl)
                ),
            ));
            steps.push(LiveTestStep::command(
                "Import exact Codex usage records",
                format!(
                    "{} import-usage --source {} --event-log {}",
                    shell_quote(&request.tokmeter_bin),
                    shell_quote_path(&codex_jsonl),
                    shell_quote_path(&event_log)
                ),
            ));
            steps.extend(report_steps(request, &event_log, &report_dir));
            notes.push("Requires an authenticated `codex` CLI.".to_owned());
        }
        LiveTestSurface::CodexTuiApi => {
            steps.push(env_required_step("OPENAI_API_KEY"));
            steps.push(init_step(request, &repo));
            steps.push(LiveTestStep::command(
                "Launch interactive Codex through the API proxy",
                format!(
                    "cd {} && {} codex-tui --provider api --keep-openai-api-key",
                    shell_quote_path(&repo),
                    shell_quote(&request.tokmeter_bin)
                ),
            ));
            steps.extend(report_steps(request, &event_log, &report_dir));
            notes.push(
                "This is an interactive live session; run the report after exiting Codex."
                    .to_owned(),
            );
        }
        LiveTestSurface::CodexTuiSubscription => {
            steps.push(init_step(request, &repo));
            steps.push(LiveTestStep::command(
                "Launch interactive Codex through subscription auth",
                format!(
                    "cd {} && {} codex-tui",
                    shell_quote_path(&repo),
                    shell_quote(&request.tokmeter_bin)
                ),
            ));
            steps.extend(report_steps(request, &event_log, &report_dir));
            notes.push(
                "Exact token fields depend on what the subscription transport exposes.".to_owned(),
            );
        }
        LiveTestSurface::ClaudeCodeApi => {
            steps.push(env_required_step("ANTHROPIC_API_KEY"));
            steps.push(init_step(request, &repo));
            steps.push(LiveTestStep::command(
                "Launch Claude Code through the Anthropic API proxy",
                format!(
                    "cd {} && {} claude-code --keep-anthropic-api-key",
                    shell_quote_path(&repo),
                    shell_quote(&request.tokmeter_bin)
                ),
            ));
            steps.extend(report_steps(request, &event_log, &report_dir));
            notes.push(
                "This is an interactive live session; run the report after exiting Claude Code."
                    .to_owned(),
            );
        }
        LiveTestSurface::ClaudeCodeSubscription => {
            steps.push(init_step(request, &repo));
            steps.push(LiveTestStep::command(
                "Launch Claude Code through subscription auth",
                format!(
                    "cd {} && {} claude-code",
                    shell_quote_path(&repo),
                    shell_quote(&request.tokmeter_bin)
                ),
            ));
            steps.extend(report_steps(request, &event_log, &report_dir));
            notes.push(
                "Exact token fields depend on what Claude Code exposes in this auth mode."
                    .to_owned(),
            );
        }
        LiveTestSurface::ClaudeDesktopConfig => {
            steps.push(LiveTestStep::command(
                "Verify the vc-tokmeter binary Claude Desktop will run",
                binary_check_command(&request.tokmeter_bin),
            ));
            steps.push(LiveTestStep::output(
                "Add this MCP config to Claude Desktop",
                claude_desktop_config(&request.tokmeter_bin, &repo, &event_log),
            ));
            notes.push(
                "After restarting Claude Desktop, ask it to use tokmeter_git_status and tokmeter_git_diff."
                    .to_owned(),
            );
        }
        LiveTestSurface::Report => {
            steps.extend(report_steps(request, &event_log, &report_dir));
        }
    }

    LiveTestPlan {
        surface: request.surface,
        repo,
        event_log,
        report_dir,
        codex_jsonl,
        steps,
        notes,
    }
}

pub fn render_live_test_plan(plan: &LiveTestPlan) -> String {
    let mut out = String::new();
    out.push_str(&format!("surface={}\n", plan.surface));
    out.push_str(&format!("repo={}\n", plan.repo.display()));
    out.push_str(&format!("event_log={}\n", plan.event_log.display()));
    out.push_str(&format!("report_dir={}\n", plan.report_dir.display()));
    if plan.surface == LiveTestSurface::CodexExec {
        out.push_str(&format!("codex_jsonl={}\n", plan.codex_jsonl.display()));
    }
    out.push('\n');

    for (index, step) in plan.steps.iter().enumerate() {
        out.push_str(&format!("{}. {}\n", index + 1, step.title));
        if let Some(command) = &step.command {
            out.push_str("   ");
            out.push_str(command);
            out.push('\n');
        }
        if let Some(output) = &step.output {
            for line in output.lines() {
                out.push_str("   ");
                out.push_str(line);
                out.push('\n');
            }
        }
    }

    if !plan.notes.is_empty() {
        out.push_str("\nNotes:\n");
        for note in &plan.notes {
            out.push_str("- ");
            out.push_str(note);
            out.push('\n');
        }
    }

    out
}

pub fn render_live_test_usage() -> String {
    let mut out = String::new();
    out.push_str("Usage:\n");
    out.push_str(
        "  vc-tokmeter live-test <surface> [--repo PATH] [--prompt TEXT] [--tokmeter-bin CMD]\n\n",
    );
    out.push_str("Surfaces:\n");
    for surface in LiveTestSurface::all() {
        out.push_str("  ");
        out.push_str(surface.as_str());
        out.push('\n');
    }
    out
}

fn init_step(request: &LiveTestRequest, repo: &Path) -> LiveTestStep {
    LiveTestStep::command(
        "Initialize tokmeter capture in the target repo",
        format!(
            "cd {} && {} init",
            shell_quote_path(repo),
            shell_quote(&request.tokmeter_bin)
        ),
    )
}

fn env_required_step(name: &str) -> LiveTestStep {
    LiveTestStep::command(
        format!("Verify {name} is set"),
        format!("test -n \"${name}\""),
    )
}

fn report_steps(
    request: &LiveTestRequest,
    event_log: &Path,
    report_dir: &Path,
) -> Vec<LiveTestStep> {
    vec![
        LiveTestStep::command(
            "Generate the tokmeter report",
            format!(
                "{} report --event-log {} --out {}",
                shell_quote(&request.tokmeter_bin),
                shell_quote_path(event_log),
                shell_quote_path(report_dir)
            ),
        ),
        LiveTestStep::command(
            "Print the live-test summary lines",
            format!(
                "rg -n 'Total tokens|Session git token share|Git tokens|Git token share|Token fidelity|codex exec exact usage|proxy exact usage|proxy estimate|mcp tool|hook' {}",
                shell_quote_path(&report_dir.join("report.md"))
            ),
        ),
    ]
}

fn claude_desktop_config(tokmeter_bin: &str, repo: &Path, event_log: &Path) -> String {
    format!(
        concat!(
            "{{\n",
            "  \"mcpServers\": {{\n",
            "    \"tokmeter-git\": {{\n",
            "      \"command\": {},\n",
            "      \"args\": [\n",
            "        \"mcp-git\",\n",
            "        \"--workdir\",\n",
            "        {},\n",
            "        \"--event-log\",\n",
            "        {}\n",
            "      ]\n",
            "    }}\n",
            "  }}\n",
            "}}"
        ),
        json_string(tokmeter_bin),
        json_string(&repo.display().to_string()),
        json_string(&event_log.display().to_string())
    )
}

fn binary_check_command(tokmeter_bin: &str) -> String {
    if tokmeter_bin.contains('/') {
        format!("test -x {}", shell_quote(tokmeter_bin))
    } else {
        format!("command -v {}", shell_quote(tokmeter_bin))
    }
}

fn shell_quote_path(path: &Path) -> String {
    shell_quote(&path.display().to_string())
}

fn shell_quote(value: &str) -> String {
    if value.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-' | b':')
    }) {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(surface: LiveTestSurface) -> LiveTestRequest {
        LiveTestRequest::new(surface, "/tmp/repo").with_tokmeter_bin("/opt/bin/vc-tokmeter")
    }

    #[test]
    fn parses_all_documented_surfaces() {
        for surface in LiveTestSurface::all() {
            assert_eq!(surface.as_str().parse::<LiveTestSurface>(), Ok(*surface));
        }
    }

    #[test]
    fn codex_exec_plan_imports_structured_usage_and_reports() {
        let plan =
            plan_live_test(&request(LiveTestSurface::CodexExec).with_prompt("Inspect git status"));
        let rendered = render_live_test_plan(&plan);

        assert!(rendered.contains("codex exec --json 'Inspect git status'"));
        assert!(rendered.contains("import-usage --source /tmp/repo/.tokmeter/codex-exec.jsonl"));
        assert!(rendered.contains("report --event-log /tmp/repo/.tokmeter/events.jsonl"));
    }

    #[test]
    fn codex_tui_api_plan_requires_openai_key_and_keeps_it() {
        let plan = plan_live_test(&request(LiveTestSurface::CodexTuiApi));
        let rendered = render_live_test_plan(&plan);

        assert!(rendered.contains("test -n \"$OPENAI_API_KEY\""));
        assert!(rendered.contains("codex-tui --provider api --keep-openai-api-key"));
    }

    #[test]
    fn claude_code_subscription_plan_removes_api_key_by_using_default_wrapper() {
        let plan = plan_live_test(&request(LiveTestSurface::ClaudeCodeSubscription));
        let rendered = render_live_test_plan(&plan);

        assert!(rendered.contains("claude-code"));
        assert!(!rendered.contains("--keep-anthropic-api-key"));
    }

    #[test]
    fn claude_desktop_config_contains_mcp_git_wiring() {
        let plan = plan_live_test(&request(LiveTestSurface::ClaudeDesktopConfig));
        let rendered = render_live_test_plan(&plan);

        assert!(rendered.contains("\"tokmeter-git\""));
        assert!(rendered.contains("\"mcp-git\""));
        assert!(rendered.contains("\"/tmp/repo/.tokmeter/events.jsonl\""));
        assert!(rendered.contains("test -x /opt/bin/vc-tokmeter"));
    }

    #[test]
    fn usage_lists_expected_surfaces() {
        let usage = render_live_test_usage();

        assert!(usage.contains("codex-tui-subscription"));
        assert!(usage.contains("claude-desktop-config"));
        assert!(usage.contains("report"));
    }
}
