use std::env;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupRequest {
    pub repo_root: PathBuf,
    pub tokmeter_bin: String,
    pub upload: UploadSetup,
    pub detection: SurfaceDetection,
}

impl SetupRequest {
    pub fn local_only(
        repo_root: impl Into<PathBuf>,
        tokmeter_bin: impl Into<String>,
        detection: SurfaceDetection,
    ) -> Self {
        Self {
            repo_root: repo_root.into(),
            tokmeter_bin: tokmeter_bin.into(),
            upload: UploadSetup::LocalOnly,
            detection,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadSetup {
    LocalOnly,
    OptIn { endpoint: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SurfaceDetection {
    pub codex_cli: ToolAvailability,
    pub claude_code_cli: ToolAvailability,
    pub claude_desktop: DesktopAvailability,
}

impl SurfaceDetection {
    pub fn with_codex_cli(mut self, availability: ToolAvailability) -> Self {
        self.codex_cli = availability;
        self
    }

    pub fn with_claude_code_cli(mut self, availability: ToolAvailability) -> Self {
        self.claude_code_cli = availability;
        self
    }

    pub fn with_claude_desktop(mut self, availability: DesktopAvailability) -> Self {
        self.claude_desktop = availability;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolAvailability {
    Available { command: String },
    Missing,
}

impl Default for ToolAvailability {
    fn default() -> Self {
        Self::Missing
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopAvailability {
    Available { config_path: PathBuf },
    MissingConfig,
}

impl Default for DesktopAvailability {
    fn default() -> Self {
        Self::MissingConfig
    }
}

pub trait SetupEnvironment {
    fn command_exists(&self, command: &str) -> bool;
    fn path_exists(&self, path: &Path) -> bool;
    fn home_dir(&self) -> Option<PathBuf>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FsSetupEnvironment;

impl SetupEnvironment for FsSetupEnvironment {
    fn command_exists(&self, command: &str) -> bool {
        command_in_path(command)
    }

    fn path_exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn home_dir(&self) -> Option<PathBuf> {
        env::var_os("HOME").map(PathBuf::from)
    }
}

pub fn detect_surfaces(environment: &impl SetupEnvironment) -> SurfaceDetection {
    SurfaceDetection {
        codex_cli: detect_command(environment, "codex"),
        claude_code_cli: detect_command(environment, "claude"),
        claude_desktop: detect_claude_desktop(environment),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupPlan {
    pub repo_root: PathBuf,
    pub collection_mode: CollectionMode,
    pub changes: Vec<SetupChange>,
    pub surfaces: Vec<SurfacePlan>,
    pub next_commands: Vec<NextCommand>,
    pub undo_commands: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CollectionMode {
    LocalOnly,
    UploadConfigured { endpoint: String },
}

impl fmt::Display for CollectionMode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LocalOnly => formatter.write_str("local-only"),
            Self::UploadConfigured { endpoint } => {
                write!(
                    formatter,
                    "local capture with opt-in upload config for {endpoint}"
                )
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupChange {
    pub path: PathBuf,
    pub action: SetupAction,
    pub detail: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupAction {
    CreateDirectory,
    CreateFile,
    UpdateFile,
    ManualEdit,
}

impl fmt::Display for SetupAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::CreateDirectory => "create directory",
            Self::CreateFile => "create file",
            Self::UpdateFile => "update file",
            Self::ManualEdit => "manual edit",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SurfacePlan {
    pub surface: AgentSurface,
    pub status: SurfaceStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentSurface {
    CodexCli,
    ClaudeCodeCli,
    ClaudeDesktop,
}

impl AgentSurface {
    pub fn label(self) -> &'static str {
        match self {
            Self::CodexCli => "Codex CLI",
            Self::ClaudeCodeCli => "Claude Code CLI",
            Self::ClaudeDesktop => "Claude Desktop",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceStatus {
    Ready,
    ManualSetup,
    Unavailable,
}

impl fmt::Display for SurfaceStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Ready => "ready",
            Self::ManualSetup => "manual setup",
            Self::Unavailable => "unavailable",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NextCommand {
    pub surface: Option<AgentSurface>,
    pub command: String,
    pub detail: &'static str,
}

fn detect_command(environment: &impl SetupEnvironment, command: &str) -> ToolAvailability {
    if environment.command_exists(command) {
        ToolAvailability::Available {
            command: command.to_owned(),
        }
    } else {
        ToolAvailability::Missing
    }
}

fn detect_claude_desktop(environment: &impl SetupEnvironment) -> DesktopAvailability {
    let Some(home_dir) = environment.home_dir() else {
        return DesktopAvailability::MissingConfig;
    };

    let config_path = home_dir
        .join("Library")
        .join("Application Support")
        .join("Claude")
        .join("claude_desktop_config.json");

    if environment.path_exists(&config_path) {
        DesktopAvailability::Available { config_path }
    } else {
        DesktopAvailability::MissingConfig
    }
}

pub fn plan_setup(request: &SetupRequest) -> SetupPlan {
    let event_log = request.repo_root.join(".tokmeter").join("events.jsonl");
    let report_dir = request.repo_root.join(".tokmeter").join("report");
    let tokmeter_dir = request.repo_root.join(".tokmeter");
    let codex_hooks = request.repo_root.join(".codex").join("hooks.json");
    let claude_hook = tokmeter_dir.join("hooks").join("claude-code-hook.json");
    let upload_config = tokmeter_dir.join("upload.json");

    let mut changes = vec![
        SetupChange {
            path: tokmeter_dir.clone(),
            action: SetupAction::CreateDirectory,
            detail: "local metrics state",
        },
        SetupChange {
            path: tokmeter_dir.join("hooks"),
            action: SetupAction::CreateDirectory,
            detail: "hook definitions",
        },
        SetupChange {
            path: codex_hooks,
            action: SetupAction::CreateFile,
            detail: "Codex hook capture wiring",
        },
        SetupChange {
            path: claude_hook,
            action: SetupAction::CreateFile,
            detail: "Claude Code hook capture wiring",
        },
    ];

    let collection_mode = match &request.upload {
        UploadSetup::LocalOnly => CollectionMode::LocalOnly,
        UploadSetup::OptIn { endpoint } => {
            changes.push(SetupChange {
                path: upload_config,
                action: SetupAction::CreateFile,
                detail: "opt-in upload endpoint and token config",
            });
            CollectionMode::UploadConfigured {
                endpoint: endpoint.clone(),
            }
        }
    };

    let mut surfaces = Vec::new();
    let mut next_commands = vec![NextCommand {
        surface: None,
        command: format!(
            "{} report --event-log {} --out {}",
            request.tokmeter_bin,
            shell_path(&event_log),
            shell_path(&report_dir)
        ),
        detail: "render local report after a measured session",
    }];

    surfaces.push(plan_codex_surface(request, &mut next_commands));
    surfaces.push(plan_claude_code_surface(request, &mut next_commands));
    surfaces.push(plan_claude_desktop_surface(
        request,
        &mut changes,
        &mut next_commands,
    ));

    SetupPlan {
        repo_root: request.repo_root.clone(),
        collection_mode,
        changes,
        surfaces,
        next_commands,
        undo_commands: vec![format!("{} uninstall", request.tokmeter_bin)],
    }
}

pub fn render_setup_plan(plan: &SetupPlan) -> String {
    let mut out = String::new();
    out.push_str("vc-tokmeter setup plan\n\n");
    out.push_str("Repository:\n");
    out.push_str("  - ");
    out.push_str(&shell_path(&plan.repo_root));
    out.push('\n');

    out.push_str("\nCollection:\n");
    out.push_str("  - mode: ");
    out.push_str(&plan.collection_mode.to_string());
    out.push('\n');
    out.push_str("  - setup never uploads automatically\n");
    if matches!(
        plan.collection_mode,
        CollectionMode::UploadConfigured { .. }
    ) {
        out.push_str("  - first upload step is a dry-run review; live upload requires --yes\n");
    }

    out.push_str("\nDetected surfaces:\n");
    for surface in &plan.surfaces {
        out.push_str("  - ");
        out.push_str(surface.surface.label());
        out.push_str(": ");
        out.push_str(&surface.status.to_string());
        out.push_str(" (");
        out.push_str(&surface.detail);
        out.push_str(")\n");
    }

    out.push_str("\nFiles/configs changed or requiring manual setup:\n");
    for change in &plan.changes {
        out.push_str("  - ");
        out.push_str(&change.action.to_string());
        out.push_str(": ");
        out.push_str(&shell_path(&change.path));
        out.push_str(" (");
        out.push_str(change.detail);
        out.push_str(")\n");
    }

    out.push_str("\nNext commands:\n");
    for command in &plan.next_commands {
        out.push_str("  - ");
        if let Some(surface) = command.surface {
            out.push_str(surface.label());
            out.push_str(": ");
        }
        out.push_str(&command.command);
        out.push_str(" (");
        out.push_str(command.detail);
        out.push_str(")\n");
    }

    out.push_str("\nUndo:\n");
    for command in &plan.undo_commands {
        out.push_str("  - ");
        out.push_str(command);
        out.push('\n');
    }

    out
}

fn plan_codex_surface(request: &SetupRequest, next_commands: &mut Vec<NextCommand>) -> SurfacePlan {
    match &request.detection.codex_cli {
        ToolAvailability::Available { command } => {
            next_commands.push(NextCommand {
                surface: Some(AgentSurface::CodexCli),
                command: format!("{} codex-tui", request.tokmeter_bin),
                detail: "launch measured interactive Codex session",
            });
            SurfacePlan {
                surface: AgentSurface::CodexCli,
                status: SurfaceStatus::Ready,
                detail: format!("found `{command}` on PATH"),
            }
        }
        ToolAvailability::Missing => SurfacePlan {
            surface: AgentSurface::CodexCli,
            status: SurfaceStatus::Unavailable,
            detail: "install Codex CLI to measure interactive Codex sessions".to_owned(),
        },
    }
}

fn plan_claude_code_surface(
    request: &SetupRequest,
    next_commands: &mut Vec<NextCommand>,
) -> SurfacePlan {
    match &request.detection.claude_code_cli {
        ToolAvailability::Available { command } => {
            next_commands.push(NextCommand {
                surface: Some(AgentSurface::ClaudeCodeCli),
                command: format!("{} claude-code", request.tokmeter_bin),
                detail: "launch measured interactive Claude Code session",
            });
            SurfacePlan {
                surface: AgentSurface::ClaudeCodeCli,
                status: SurfaceStatus::Ready,
                detail: format!("found `{command}` on PATH"),
            }
        }
        ToolAvailability::Missing => SurfacePlan {
            surface: AgentSurface::ClaudeCodeCli,
            status: SurfaceStatus::Unavailable,
            detail: "install Claude Code CLI to measure Claude Code sessions".to_owned(),
        },
    }
}

fn plan_claude_desktop_surface(
    request: &SetupRequest,
    changes: &mut Vec<SetupChange>,
    next_commands: &mut Vec<NextCommand>,
) -> SurfacePlan {
    match &request.detection.claude_desktop {
        DesktopAvailability::Available { config_path } => {
            changes.push(SetupChange {
                path: config_path.clone(),
                action: SetupAction::ManualEdit,
                detail: "add tokmeter MCP git server block",
            });
            next_commands.push(NextCommand {
                surface: Some(AgentSurface::ClaudeDesktop),
                command: format!(
                    "{} mcp-git --workdir {} --event-log {}",
                    request.tokmeter_bin,
                    shell_path(&request.repo_root),
                    shell_path(&request.repo_root.join(".tokmeter").join("events.jsonl"))
                ),
                detail: "command to place in Claude Desktop MCP config",
            });
            SurfacePlan {
                surface: AgentSurface::ClaudeDesktop,
                status: SurfaceStatus::ManualSetup,
                detail: format!("config can be edited at {}", shell_path(config_path)),
            }
        }
        DesktopAvailability::MissingConfig => SurfacePlan {
            surface: AgentSurface::ClaudeDesktop,
            status: SurfaceStatus::Unavailable,
            detail: "Claude Desktop config was not found; skip desktop MCP setup".to_owned(),
        },
    }
}

fn shell_path(path: &Path) -> String {
    let raw = path.display().to_string();
    if raw.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':' | '=' | '+')
    }) {
        raw
    } else {
        let escaped = raw.replace('\'', "'\\''");
        format!("'{escaped}'")
    }
}

fn command_in_path(command: &str) -> bool {
    let command_path = Path::new(command);
    if command_path.components().count() > 1 {
        return command_path.exists();
    }

    let Some(paths) = env::var_os("PATH") else {
        return false;
    };

    env::split_paths(&paths).any(|path| path.join(command).exists())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn request(detection: SurfaceDetection) -> SetupRequest {
        SetupRequest::local_only("/tmp/example repo", "vc-tokmeter", detection)
    }

    #[derive(Default)]
    struct FakeEnvironment {
        commands: BTreeSet<String>,
        paths: BTreeSet<PathBuf>,
        home_dir: Option<PathBuf>,
    }

    impl FakeEnvironment {
        fn with_command(mut self, command: &str) -> Self {
            self.commands.insert(command.to_owned());
            self
        }

        fn with_path(mut self, path: &str) -> Self {
            self.paths.insert(PathBuf::from(path));
            self
        }

        fn with_home_dir(mut self, path: &str) -> Self {
            self.home_dir = Some(PathBuf::from(path));
            self
        }
    }

    impl SetupEnvironment for FakeEnvironment {
        fn command_exists(&self, command: &str) -> bool {
            self.commands.contains(command)
        }

        fn path_exists(&self, path: &Path) -> bool {
            self.paths.contains(path)
        }

        fn home_dir(&self) -> Option<PathBuf> {
            self.home_dir.clone()
        }
    }

    #[test]
    fn detect_surfaces_finds_cli_tools_and_claude_desktop_config() {
        let environment = FakeEnvironment::default()
            .with_command("codex")
            .with_command("claude")
            .with_home_dir("/Users/alice")
            .with_path(
                "/Users/alice/Library/Application Support/Claude/claude_desktop_config.json",
            );

        let detection = detect_surfaces(&environment);

        assert_eq!(
            detection.codex_cli,
            ToolAvailability::Available {
                command: "codex".to_owned()
            }
        );
        assert_eq!(
            detection.claude_code_cli,
            ToolAvailability::Available {
                command: "claude".to_owned()
            }
        );
        assert!(matches!(
            detection.claude_desktop,
            DesktopAvailability::Available { .. }
        ));
    }

    #[test]
    fn setup_defaults_to_local_only_without_upload() {
        let plan = plan_setup(&request(SurfaceDetection::default()));

        assert_eq!(plan.collection_mode, CollectionMode::LocalOnly);
        assert!(!plan.changes.iter().any(|change| {
            change.path.file_name().and_then(|name| name.to_str()) == Some("upload.json")
        }));

        let rendered = render_setup_plan(&plan);
        assert!(rendered.contains("mode: local-only"));
        assert!(rendered.contains("setup never uploads automatically"));
    }

    #[test]
    fn setup_detects_available_cli_surfaces_and_prints_next_commands() {
        let detection = SurfaceDetection::default()
            .with_codex_cli(ToolAvailability::Available {
                command: "codex".to_owned(),
            })
            .with_claude_code_cli(ToolAvailability::Available {
                command: "claude".to_owned(),
            });

        let plan = plan_setup(&request(detection));

        assert!(plan.surfaces.iter().any(|surface| {
            surface.surface == AgentSurface::CodexCli && surface.status == SurfaceStatus::Ready
        }));
        assert!(plan.surfaces.iter().any(|surface| {
            surface.surface == AgentSurface::ClaudeCodeCli && surface.status == SurfaceStatus::Ready
        }));
        assert!(
            plan.next_commands
                .iter()
                .any(|command| command.command == "vc-tokmeter codex-tui")
        );
        assert!(
            plan.next_commands
                .iter()
                .any(|command| command.command == "vc-tokmeter claude-code")
        );
    }

    #[test]
    fn setup_includes_claude_desktop_manual_mcp_step_when_config_exists() {
        let detection =
            SurfaceDetection::default().with_claude_desktop(DesktopAvailability::Available {
                config_path: PathBuf::from(
                    "/Users/alice/Library/Application Support/Claude/claude_desktop_config.json",
                ),
            });

        let plan = plan_setup(&request(detection));

        assert!(plan.surfaces.iter().any(|surface| {
            surface.surface == AgentSurface::ClaudeDesktop
                && surface.status == SurfaceStatus::ManualSetup
        }));
        assert!(plan.changes.iter().any(|change| {
            change.action == SetupAction::ManualEdit
                && change.path.ends_with("claude_desktop_config.json")
        }));

        let rendered = render_setup_plan(&plan);
        assert!(rendered.contains("Claude Desktop: manual setup"));
        assert!(rendered.contains("mcp-git --workdir '/tmp/example repo'"));
    }

    #[test]
    fn setup_can_plan_upload_opt_in_config_without_uploading() {
        let mut setup = request(SurfaceDetection::default());
        setup.upload = UploadSetup::OptIn {
            endpoint: "https://collector.example.test".to_owned(),
        };

        let plan = plan_setup(&setup);

        assert_eq!(
            plan.collection_mode,
            CollectionMode::UploadConfigured {
                endpoint: "https://collector.example.test".to_owned()
            }
        );
        assert!(plan.changes.iter().any(|change| {
            change.path.ends_with(".tokmeter/upload.json")
                && change.detail == "opt-in upload endpoint and token config"
        }));

        let rendered = render_setup_plan(&plan);
        assert!(rendered.contains("setup never uploads automatically"));
        assert!(rendered.contains("first upload step is a dry-run review"));
        assert!(rendered.contains("opt-in upload config"));
    }

    #[test]
    fn setup_reports_unavailable_surfaces_with_actionable_details() {
        let plan = plan_setup(&request(SurfaceDetection::default()));

        assert_eq!(
            plan.surfaces
                .iter()
                .filter(|surface| surface.status == SurfaceStatus::Unavailable)
                .count(),
            3
        );

        let rendered = render_setup_plan(&plan);
        assert!(rendered.contains("install Codex CLI"));
        assert!(rendered.contains("install Claude Code CLI"));
        assert!(rendered.contains("Claude Desktop config was not found"));
    }
}
