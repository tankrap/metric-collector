use std::fmt;
use std::path::{Path, PathBuf};

pub const MANAGED_BLOCK_BEGIN: &str = "# vc-tokmeter managed begin";
pub const MANAGED_BLOCK_END: &str = "# vc-tokmeter managed end";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitRequest {
    pub install_root: PathBuf,
    pub agent_config_path: PathBuf,
    pub proxy_base_url: String,
}

impl InitRequest {
    pub fn new(
        install_root: impl Into<PathBuf>,
        agent_config_path: impl Into<PathBuf>,
        proxy_base_url: impl Into<String>,
    ) -> Result<Self, InstallError> {
        let request = Self {
            install_root: install_root.into(),
            agent_config_path: agent_config_path.into(),
            proxy_base_url: proxy_base_url.into(),
        };
        request.validate()?;
        Ok(request)
    }

    fn validate(&self) -> Result<(), InstallError> {
        validate_path("install root", &self.install_root)?;
        validate_path("agent config path", &self.agent_config_path)?;
        validate_base_url(&self.proxy_base_url)?;
        Ok(())
    }

    fn tokmeter_dir(&self) -> PathBuf {
        self.install_root.join(".tokmeter")
    }

    fn hooks_dir(&self) -> PathBuf {
        self.tokmeter_dir().join("hooks")
    }

    fn codex_dir(&self) -> PathBuf {
        self.install_root.join(".codex")
    }

    fn runs_dir(&self) -> PathBuf {
        self.tokmeter_dir().join("runs")
    }

    fn manifest_path(&self) -> PathBuf {
        self.tokmeter_dir().join("install-manifest.txt")
    }

    fn hook_path(&self) -> PathBuf {
        self.hooks_dir().join("claude-code-hook.json")
    }

    fn codex_hook_path(&self) -> PathBuf {
        self.codex_dir().join("hooks.json")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitPlan {
    pub records: Vec<InstalledItemRecord>,
}

impl InitPlan {
    pub fn changed_items(&self) -> impl Iterator<Item = &InstalledItemRecord> {
        self.records
            .iter()
            .filter(|record| record.action.changes_existing_item())
    }

    pub fn installed_items(&self) -> impl Iterator<Item = &InstalledItemRecord> {
        self.records
            .iter()
            .filter(|record| !record.action.changes_existing_item())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledItemRecord {
    pub path: PathBuf,
    pub kind: InstallItemKind,
    pub action: InstallAction,
    pub created_by_tokmeter: bool,
    pub detail: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallItemKind {
    Directory,
    File,
    AgentConfig,
}

impl fmt::Display for InstallItemKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Directory => f.write_str("directory"),
            Self::File => f.write_str("file"),
            Self::AgentConfig => f.write_str("agent config"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallAction {
    CreateDirectory,
    CreateFile,
    CreateManagedAgentConfig,
    UpdateManagedAgentConfig,
    ReplaceManagedAgentConfigBlock,
}

impl InstallAction {
    fn changes_existing_item(self) -> bool {
        matches!(
            self,
            Self::UpdateManagedAgentConfig | Self::ReplaceManagedAgentConfigBlock
        )
    }
}

impl fmt::Display for InstallAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CreateDirectory => f.write_str("create directory"),
            Self::CreateFile => f.write_str("create file"),
            Self::CreateManagedAgentConfig => f.write_str("create managed agent config"),
            Self::UpdateManagedAgentConfig => f.write_str("add managed config block"),
            Self::ReplaceManagedAgentConfigBlock => f.write_str("replace managed config block"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UninstallPlan {
    pub removals: Vec<RemovalRecord>,
    pub mode: UninstallMode,
}

impl UninstallPlan {
    pub fn is_residue_free_for_tokmeter_items(&self) -> bool {
        self.removals
            .iter()
            .all(|record| record.source_created_by_tokmeter)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovalRecord {
    pub path: PathBuf,
    pub kind: InstallItemKind,
    pub action: RemovalAction,
    pub source_created_by_tokmeter: bool,
    pub detail: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovalAction {
    DeleteFile,
    DeleteDirectoryIfEmpty,
    RemoveManagedAgentConfigBlock,
}

impl fmt::Display for RemovalAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DeleteFile => f.write_str("delete file"),
            Self::DeleteDirectoryIfEmpty => f.write_str("delete directory if empty"),
            Self::RemoveManagedAgentConfigBlock => f.write_str("remove managed config block"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UninstallMode {
    FullUninstall,
    FailedInitCleanup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigAfterUninstall {
    Removed,
    Present(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    EmptyPath(&'static str),
    RelativePath(&'static str),
    EmptyBaseUrl,
    BaseUrlContainsCredentials,
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPath(label) => write!(f, "{label} must not be empty"),
            Self::RelativePath(label) => write!(f, "{label} must be absolute"),
            Self::EmptyBaseUrl => f.write_str("proxy base URL must not be empty"),
            Self::BaseUrlContainsCredentials => {
                f.write_str("proxy base URL must not include credentials")
            }
        }
    }
}

impl std::error::Error for InstallError {}

pub fn plan_init(
    request: &InitRequest,
    existing_agent_config: Option<&str>,
) -> Result<InitPlan, InstallError> {
    request.validate()?;

    let agent_config_action = match existing_agent_config {
        None => InstallAction::CreateManagedAgentConfig,
        Some(config) if contains_managed_block(config) => {
            InstallAction::ReplaceManagedAgentConfigBlock
        }
        Some(_) => InstallAction::UpdateManagedAgentConfig,
    };

    Ok(InitPlan {
        records: vec![
            InstalledItemRecord {
                path: request.tokmeter_dir(),
                kind: InstallItemKind::Directory,
                action: InstallAction::CreateDirectory,
                created_by_tokmeter: true,
                detail: "local tokmeter state root",
            },
            InstalledItemRecord {
                path: request.hooks_dir(),
                kind: InstallItemKind::Directory,
                action: InstallAction::CreateDirectory,
                created_by_tokmeter: true,
                detail: "hook definitions",
            },
            InstalledItemRecord {
                path: request.runs_dir(),
                kind: InstallItemKind::Directory,
                action: InstallAction::CreateDirectory,
                created_by_tokmeter: true,
                detail: "local run metadata",
            },
            InstalledItemRecord {
                path: request.codex_dir(),
                kind: InstallItemKind::Directory,
                action: InstallAction::CreateDirectory,
                created_by_tokmeter: true,
                detail: "Codex project configuration",
            },
            InstalledItemRecord {
                path: request.manifest_path(),
                kind: InstallItemKind::File,
                action: InstallAction::CreateFile,
                created_by_tokmeter: true,
                detail: "install manifest for residue-free uninstall",
            },
            InstalledItemRecord {
                path: request.hook_path(),
                kind: InstallItemKind::File,
                action: InstallAction::CreateFile,
                created_by_tokmeter: true,
                detail: "agent hook wiring",
            },
            InstalledItemRecord {
                path: request.codex_hook_path(),
                kind: InstallItemKind::File,
                action: InstallAction::CreateFile,
                created_by_tokmeter: true,
                detail: "Codex project hook wiring",
            },
            InstalledItemRecord {
                path: request.agent_config_path.clone(),
                kind: InstallItemKind::AgentConfig,
                action: agent_config_action,
                created_by_tokmeter: matches!(
                    agent_config_action,
                    InstallAction::CreateManagedAgentConfig
                ),
                detail: "managed block points the agent at the tokmeter proxy",
            },
        ],
    })
}

pub fn plan_uninstall(
    records: &[InstalledItemRecord],
    current_agent_config: Option<&str>,
) -> UninstallPlan {
    uninstall_plan(records, current_agent_config, UninstallMode::FullUninstall)
}

pub fn plan_failed_init_cleanup(
    partial_records: &[InstalledItemRecord],
    current_agent_config: Option<&str>,
) -> UninstallPlan {
    uninstall_plan(
        partial_records,
        current_agent_config,
        UninstallMode::FailedInitCleanup,
    )
}

pub fn agent_config_after_init(existing: Option<&str>, request: &InitRequest) -> String {
    let block = managed_block(request);
    match existing {
        None => format!("{block}\n"),
        Some(config) if contains_managed_block(config) => replace_managed_block(config, &block),
        Some(config) => append_managed_block(config, &block),
    }
}

pub fn agent_config_after_uninstall(
    config_before_uninstall: Option<&str>,
    records: &[InstalledItemRecord],
) -> Option<ConfigAfterUninstall> {
    let current = config_before_uninstall?;
    let config_record = records
        .iter()
        .find(|record| record.kind == InstallItemKind::AgentConfig)?;
    let unmanaged = remove_managed_block(current);

    if config_record.created_by_tokmeter && unmanaged.trim().is_empty() {
        Some(ConfigAfterUninstall::Removed)
    } else {
        Some(ConfigAfterUninstall::Present(unmanaged))
    }
}

pub fn render_init_summary(plan: &InitPlan) -> String {
    let mut out = String::new();
    out.push_str("vc-tokmeter init plan\n\n");
    out.push_str("Installed items:\n");
    for record in plan.installed_items() {
        write_record_line(&mut out, record);
    }

    out.push_str("\nChanged items:\n");
    let changed: Vec<_> = plan.changed_items().collect();
    if changed.is_empty() {
        out.push_str("  - none\n");
    } else {
        for record in changed {
            write_record_line(&mut out, record);
        }
    }

    out.push_str("\nRemoval instructions:\n");
    out.push_str("  - Run `vc-tokmeter uninstall` to remove tokmeter-created artifacts.\n");
    out.push_str("  - Manual cleanup removes only paths listed as created by tokmeter.\n");
    out.push_str("  - Remove the managed block between these markers from agent config files:\n");
    out.push_str("    ");
    out.push_str(MANAGED_BLOCK_BEGIN);
    out.push('\n');
    out.push_str("    ");
    out.push_str(MANAGED_BLOCK_END);
    out.push('\n');
    out
}

pub fn render_uninstall_summary(plan: &UninstallPlan) -> String {
    let mut out = String::new();
    match plan.mode {
        UninstallMode::FullUninstall => out.push_str("vc-tokmeter uninstall plan\n\n"),
        UninstallMode::FailedInitCleanup => {
            out.push_str("vc-tokmeter failed-init cleanup plan\n\n")
        }
    }

    out.push_str("Removals:\n");
    if plan.removals.is_empty() {
        out.push_str("  - none\n");
    } else {
        for record in &plan.removals {
            out.push_str("  - ");
            out.push_str(&record.action.to_string());
            out.push_str(": ");
            out.push_str(&record.path.display().to_string());
            out.push_str(" (");
            out.push_str(record.detail);
            out.push_str(")\n");
        }
    }

    out.push_str("\nSafety:\n");
    out.push_str("  - Only records marked as tokmeter-created are eligible for deletion.\n");
    out.push_str("  - Existing agent config keeps all content outside the managed block.\n");
    out
}

fn uninstall_plan(
    records: &[InstalledItemRecord],
    current_agent_config: Option<&str>,
    mode: UninstallMode,
) -> UninstallPlan {
    let mut removals = Vec::new();

    for record in records.iter().rev() {
        match record.kind {
            InstallItemKind::Directory if record.created_by_tokmeter => {
                removals.push(RemovalRecord {
                    path: record.path.clone(),
                    kind: record.kind,
                    action: RemovalAction::DeleteDirectoryIfEmpty,
                    source_created_by_tokmeter: true,
                    detail: record.detail,
                });
            }
            InstallItemKind::File if record.created_by_tokmeter => {
                removals.push(RemovalRecord {
                    path: record.path.clone(),
                    kind: record.kind,
                    action: RemovalAction::DeleteFile,
                    source_created_by_tokmeter: true,
                    detail: record.detail,
                });
            }
            InstallItemKind::AgentConfig => {
                let Some(current_agent_config) = current_agent_config else {
                    continue;
                };
                let has_managed_block = contains_managed_block(current_agent_config);
                if has_managed_block {
                    let unmanaged = remove_managed_block(current_agent_config);
                    let action = if record.created_by_tokmeter && unmanaged.trim().is_empty() {
                        RemovalAction::DeleteFile
                    } else {
                        RemovalAction::RemoveManagedAgentConfigBlock
                    };
                    removals.push(RemovalRecord {
                        path: record.path.clone(),
                        kind: record.kind,
                        action,
                        source_created_by_tokmeter: true,
                        detail: record.detail,
                    });
                }
            }
            _ => {}
        }
    }

    UninstallPlan { removals, mode }
}

fn write_record_line(out: &mut String, record: &InstalledItemRecord) {
    out.push_str("  - ");
    out.push_str(&record.action.to_string());
    out.push_str(": ");
    out.push_str(&record.path.display().to_string());
    out.push_str(" (");
    out.push_str(record.detail);
    out.push_str(")\n");
}

fn managed_block(request: &InitRequest) -> String {
    format!(
        "{MANAGED_BLOCK_BEGIN}\ntokmeter_proxy_base_url = \"{}\"\ntokmeter_hook = \"{}\"\n{MANAGED_BLOCK_END}",
        request.proxy_base_url,
        request.hook_path().display()
    )
}

fn append_managed_block(existing: &str, block: &str) -> String {
    let mut output = String::with_capacity(existing.len() + block.len() + 3);
    output.push_str(existing);
    if !output.ends_with('\n') {
        output.push('\n');
    }
    output.push('\n');
    output.push_str(block);
    output.push('\n');
    output
}

fn replace_managed_block(existing: &str, block: &str) -> String {
    let without_block = remove_managed_block(existing);
    append_managed_block(without_block.trim_end_matches('\n'), block)
}

fn remove_managed_block(existing: &str) -> String {
    let mut output = String::new();
    let mut in_managed_block = false;

    for line in existing.lines() {
        if line.trim() == MANAGED_BLOCK_BEGIN {
            in_managed_block = true;
            continue;
        }

        if line.trim() == MANAGED_BLOCK_END {
            in_managed_block = false;
            continue;
        }

        if !in_managed_block {
            output.push_str(line);
            output.push('\n');
        }
    }

    collapse_extra_blank_lines(output.trim_end_matches('\n')).to_string()
}

fn collapse_extra_blank_lines(input: &str) -> String {
    let mut output = String::new();
    let mut previous_blank = false;

    for line in input.lines() {
        let blank = line.trim().is_empty();
        if blank && previous_blank {
            continue;
        }
        previous_blank = blank;
        output.push_str(line);
        output.push('\n');
    }

    output
}

fn contains_managed_block(config: &str) -> bool {
    config
        .lines()
        .any(|line| line.trim() == MANAGED_BLOCK_BEGIN)
        && config.lines().any(|line| line.trim() == MANAGED_BLOCK_END)
}

fn validate_path(label: &'static str, path: &Path) -> Result<(), InstallError> {
    if path.as_os_str().is_empty() {
        return Err(InstallError::EmptyPath(label));
    }
    if !path.is_absolute() {
        return Err(InstallError::RelativePath(label));
    }
    Ok(())
}

fn validate_base_url(base_url: &str) -> Result<(), InstallError> {
    let trimmed = base_url.trim();
    if trimmed.is_empty() {
        return Err(InstallError::EmptyBaseUrl);
    }

    if let Some(after_scheme) = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))
    {
        let authority = after_scheme.split('/').next().unwrap_or_default();
        if authority.contains('@') {
            return Err(InstallError::BaseUrlContainsCredentials);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> InitRequest {
        InitRequest::new(
            "/tmp/project",
            "/tmp/project/.claude/settings.toml",
            "http://127.0.0.1:8787",
        )
        .unwrap()
    }

    #[test]
    fn init_output_lists_records_and_removal_instructions() {
        let request = request();
        let plan = plan_init(&request, Some("theme = \"dark\"\n")).unwrap();
        let output = render_init_summary(&plan);

        assert!(output.contains("Installed items:"));
        assert!(output.contains("/tmp/project/.tokmeter"));
        assert!(output.contains("Changed items:"));
        assert!(output.contains("/tmp/project/.claude/settings.toml"));
        assert!(output.contains("vc-tokmeter uninstall"));
        assert!(output.contains("Remove the managed block"));
        assert!(output.contains(MANAGED_BLOCK_BEGIN));
        assert!(output.contains(MANAGED_BLOCK_END));
    }

    #[test]
    fn uninstall_after_full_init_removes_tokmeter_artifacts_and_created_config() {
        let request = request();
        let plan = plan_init(&request, None).unwrap();
        let config = agent_config_after_init(None, &request);
        let uninstall = plan_uninstall(&plan.records, Some(&config));
        let config_after = agent_config_after_uninstall(Some(&config), &plan.records);

        assert!(uninstall.is_residue_free_for_tokmeter_items());
        assert_eq!(
            config_after,
            Some(ConfigAfterUninstall::Removed),
            "config created solely by tokmeter should be removed"
        );
        assert!(
            uninstall
                .removals
                .iter()
                .any(|record| record.action == RemovalAction::DeleteFile
                    && record.path == request.agent_config_path)
        );
        assert!(
            uninstall
                .removals
                .iter()
                .any(|record| record.path == request.hook_path())
        );
        assert!(
            uninstall
                .removals
                .iter()
                .any(|record| record.path == request.tokmeter_dir())
        );
    }

    #[test]
    fn hook_runtime_paths_are_residue_free_through_init_uninstall_planning() {
        let request = request();
        let existing = "theme = \"dark\"\n";
        let plan = plan_init(&request, Some(existing)).unwrap();
        let config = agent_config_after_init(Some(existing), &request);
        let uninstall = plan_uninstall(&plan.records, Some(&config));

        assert!(uninstall.is_residue_free_for_tokmeter_items());

        for expected_path in [
            request.codex_hook_path(),
            request.codex_dir(),
            request.hook_path(),
            request.hooks_dir(),
            request.manifest_path(),
            request.runs_dir(),
            request.tokmeter_dir(),
        ] {
            assert!(
                uninstall
                    .removals
                    .iter()
                    .any(|record| record.path == expected_path),
                "missing uninstall removal for {}",
                expected_path.display()
            );
        }

        assert!(uninstall.removals.iter().any(|record| {
            record.path == request.agent_config_path
                && record.action == RemovalAction::RemoveManagedAgentConfigBlock
        }));
        assert_eq!(
            agent_config_after_uninstall(Some(&config), &plan.records),
            Some(ConfigAfterUninstall::Present(existing.to_string()))
        );
    }

    #[test]
    fn failed_init_cleanup_removes_only_partial_records() {
        let request = request();
        let plan = plan_init(&request, Some("existing = true\n")).unwrap();
        let partial_records = vec![
            plan.records[0].clone(),
            plan.records[1].clone(),
            plan.records[7].clone(),
        ];
        let config = agent_config_after_init(Some("existing = true\n"), &request);
        let cleanup = plan_failed_init_cleanup(&partial_records, Some(&config));
        let config_after = agent_config_after_uninstall(Some(&config), &partial_records);

        assert_eq!(cleanup.mode, UninstallMode::FailedInitCleanup);
        assert_eq!(cleanup.removals.len(), 3);
        assert!(
            cleanup
                .removals
                .iter()
                .any(|record| record.action == RemovalAction::RemoveManagedAgentConfigBlock)
        );
        assert!(
            !cleanup
                .removals
                .iter()
                .any(|record| record.path == request.hook_path())
        );
        assert_eq!(
            config_after,
            Some(ConfigAfterUninstall::Present(
                "existing = true\n".to_string()
            ))
        );
    }

    #[test]
    fn uninstall_preserves_unrelated_config_content() {
        let request = request();
        let existing = "theme = \"dark\"\nmodel = \"sonnet\"\n";
        let plan = plan_init(&request, Some(existing)).unwrap();
        let config = agent_config_after_init(Some(existing), &request);
        let config_after = agent_config_after_uninstall(Some(&config), &plan.records);

        assert_eq!(
            config_after,
            Some(ConfigAfterUninstall::Present(existing.to_string()))
        );

        let created_plan = plan_init(&request, None).unwrap();
        let mut created_config = agent_config_after_init(None, &request);
        created_config.push_str("user_added = true\n");
        let created_uninstall = plan_uninstall(&created_plan.records, Some(&created_config));
        let created_config_after =
            agent_config_after_uninstall(Some(&created_config), &created_plan.records);

        assert!(created_uninstall.removals.iter().any(|record| {
            record.path == request.agent_config_path
                && record.action == RemovalAction::RemoveManagedAgentConfigBlock
        }));
        assert_eq!(
            created_config_after,
            Some(ConfigAfterUninstall::Present(
                "user_added = true\n".to_string()
            ))
        );
    }

    #[test]
    fn rendered_output_does_not_leak_credentials_or_prompt_content() {
        let request = request();
        let existing =
            "api_key = \"sk-test-secret\"\nprompt = \"private customer source content\"\n";
        let plan = plan_init(&request, Some(existing)).unwrap();
        let config = agent_config_after_init(Some(existing), &request);
        let uninstall = plan_uninstall(&plan.records, Some(&config));
        let rendered = format!(
            "{}\n{}",
            render_init_summary(&plan),
            render_uninstall_summary(&uninstall)
        );

        assert!(!rendered.contains("sk-test-secret"));
        assert!(!rendered.contains("private customer source content"));
        assert!(!rendered.contains("api_key"));
        assert_eq!(
            InitRequest::new(
                "/tmp/project",
                "/tmp/project/.claude/settings.toml",
                "https://user:password@example.test/v1",
            )
            .unwrap_err(),
            InstallError::BaseUrlContainsCredentials
        );
    }
}
