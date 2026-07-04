use std::fmt;
use std::path::{Path, PathBuf};

const TOKMETER_MARKER_PREFIX: &str = "vc-tokmeter-hook:";
const DEFAULT_MATCHER: &str = "*";
const DEFAULT_TIMEOUT_SECONDS: u64 = 30;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ClaudeHooksConfig {
    pub groups: Vec<ClaudeHookGroup>,
}

impl ClaudeHooksConfig {
    pub fn new(groups: Vec<ClaudeHookGroup>) -> Self {
        Self { groups }
    }

    pub fn is_empty(&self) -> bool {
        self.groups.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeHookGroup {
    pub event: ClaudeHookEvent,
    pub matcher: String,
    pub hooks: Vec<ClaudeHookEntry>,
}

impl ClaudeHookGroup {
    pub fn new(
        event: ClaudeHookEvent,
        matcher: impl Into<String>,
        hooks: Vec<ClaudeHookEntry>,
    ) -> Self {
        Self {
            event,
            matcher: matcher.into(),
            hooks,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaudeHookEntry {
    pub kind: String,
    pub command: String,
    pub timeout_seconds: Option<u64>,
}

impl ClaudeHookEntry {
    pub fn command(command: impl Into<String>) -> Self {
        Self {
            kind: "command".to_owned(),
            command: command.into(),
            timeout_seconds: None,
        }
    }

    pub fn with_timeout_seconds(mut self, timeout_seconds: u64) -> Self {
        self.timeout_seconds = Some(timeout_seconds);
        self
    }

    pub fn is_tokmeter_entry_for(&self, marker: &str) -> bool {
        self.kind == "command" && self.command.contains(marker)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeHookEvent {
    PreToolUse,
    PostToolUse,
}

impl ClaudeHookEvent {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PreToolUse => "PreToolUse",
            Self::PostToolUse => "PostToolUse",
        }
    }
}

impl fmt::Display for ClaudeHookEvent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokmeterHookInstall {
    pub install_id: String,
    pub config_path: PathBuf,
    pub binary: String,
    pub matcher: String,
    pub timeout_seconds: u64,
}

impl TokmeterHookInstall {
    pub fn new(
        install_id: impl Into<String>,
        config_path: impl Into<PathBuf>,
    ) -> Result<Self, ClaudeHookError> {
        let settings = Self {
            install_id: install_id.into(),
            config_path: config_path.into(),
            binary: "vc-tokmeter".to_owned(),
            matcher: DEFAULT_MATCHER.to_owned(),
            timeout_seconds: DEFAULT_TIMEOUT_SECONDS,
        };
        settings.validate()?;
        Ok(settings)
    }

    pub fn with_binary(mut self, binary: impl Into<String>) -> Result<Self, ClaudeHookError> {
        self.binary = binary.into();
        self.validate()?;
        Ok(self)
    }

    pub fn with_matcher(mut self, matcher: impl Into<String>) -> Result<Self, ClaudeHookError> {
        self.matcher = matcher.into();
        self.validate()?;
        Ok(self)
    }

    pub fn with_timeout_seconds(mut self, timeout_seconds: u64) -> Result<Self, ClaudeHookError> {
        self.timeout_seconds = timeout_seconds;
        self.validate()?;
        Ok(self)
    }

    pub fn marker(&self) -> String {
        marker_for(&self.install_id)
    }

    fn validate(&self) -> Result<(), ClaudeHookError> {
        validate_install_id(&self.install_id)?;
        validate_command_token("binary", &self.binary)?;
        validate_matcher(&self.matcher)?;

        if self.timeout_seconds == 0 {
            return Err(ClaudeHookError::InvalidTimeout);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookInstallMetadata {
    pub install_id: String,
    pub config_path: PathBuf,
    pub installed_entries: Vec<HookInstalledEntry>,
    pub created_groups: Vec<HookGroupKey>,
}

impl HookInstallMetadata {
    pub fn markers(&self) -> Vec<&str> {
        self.installed_entries
            .iter()
            .map(|entry| entry.marker.as_str())
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookInstalledEntry {
    pub event: ClaudeHookEvent,
    pub matcher: String,
    pub command: String,
    pub marker: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookGroupKey {
    pub event: ClaudeHookEvent,
    pub matcher: String,
}

impl HookGroupKey {
    fn matches_group(&self, group: &ClaudeHookGroup) -> bool {
        self.event == group.event && self.matcher == group.matcher
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookUninstallReport {
    pub removed_entries: usize,
    pub removed_groups: usize,
}

pub fn plausible_config_paths(home_dir: impl AsRef<Path>) -> Vec<PathBuf> {
    let home_dir = home_dir.as_ref();
    vec![
        home_dir.join(".claude").join("settings.json"),
        home_dir.join(".claude").join("settings.local.json"),
        home_dir
            .join(".config")
            .join("claude-code")
            .join("settings.json"),
        home_dir
            .join("Library")
            .join("Application Support")
            .join("Claude Code")
            .join("settings.json"),
        home_dir.join(".claude.json"),
    ]
}

pub fn select_config_path(home_dir: impl AsRef<Path>, exists: impl Fn(&Path) -> bool) -> PathBuf {
    let mut paths = plausible_config_paths(home_dir);

    if let Some(existing) = paths.iter().find(|path| exists(path)).cloned() {
        return existing;
    }

    paths.remove(0)
}

pub fn tokmeter_hook_entries(
    settings: &TokmeterHookInstall,
) -> Result<Vec<(ClaudeHookEvent, ClaudeHookEntry)>, ClaudeHookError> {
    settings.validate()?;

    Ok(vec![
        (
            ClaudeHookEvent::PreToolUse,
            tokmeter_hook_entry(settings, ClaudeHookEvent::PreToolUse),
        ),
        (
            ClaudeHookEvent::PostToolUse,
            tokmeter_hook_entry(settings, ClaudeHookEvent::PostToolUse),
        ),
    ])
}

pub fn install_tokmeter_hooks(
    config: &mut ClaudeHooksConfig,
    settings: &TokmeterHookInstall,
) -> Result<HookInstallMetadata, ClaudeHookError> {
    let desired_entries = tokmeter_hook_entries(settings)?;
    let marker = settings.marker();
    let mut installed_entries = Vec::new();
    let mut created_groups = Vec::new();

    for (event, desired_entry) in desired_entries {
        let matcher = settings.matcher.clone();
        let group_index = match config
            .groups
            .iter()
            .position(|group| group.event == event && group.matcher == matcher)
        {
            Some(index) => index,
            None => {
                config
                    .groups
                    .push(ClaudeHookGroup::new(event, matcher.clone(), Vec::new()));
                created_groups.push(HookGroupKey {
                    event,
                    matcher: matcher.clone(),
                });
                config.groups.len() - 1
            }
        };

        let group = &mut config.groups[group_index];
        if let Some(existing_index) = group
            .hooks
            .iter()
            .position(|hook| hook.is_tokmeter_entry_for(&marker))
        {
            group.hooks[existing_index] = desired_entry.clone();
        } else {
            group.hooks.push(desired_entry.clone());
        }

        installed_entries.push(HookInstalledEntry {
            event,
            matcher,
            command: desired_entry.command.clone(),
            marker: marker.clone(),
        });
    }

    Ok(HookInstallMetadata {
        install_id: settings.install_id.clone(),
        config_path: settings.config_path.clone(),
        installed_entries,
        created_groups,
    })
}

pub fn uninstall_tokmeter_hooks(
    config: &mut ClaudeHooksConfig,
    metadata: &HookInstallMetadata,
) -> HookUninstallReport {
    let mut removed_entries = 0;

    for group in &mut config.groups {
        let before = group.hooks.len();
        let event = group.event;
        let matcher = group.matcher.clone();
        group.hooks.retain(|hook| {
            !metadata.installed_entries.iter().any(|entry| {
                entry.event == event
                    && entry.matcher == matcher
                    && hook.is_tokmeter_entry_for(&entry.marker)
            })
        });
        removed_entries += before - group.hooks.len();
    }

    let before_groups = config.groups.len();
    config.groups.retain(|group| {
        !group.hooks.is_empty()
            || !metadata
                .created_groups
                .iter()
                .any(|created| created.matches_group(group))
    });

    HookUninstallReport {
        removed_entries,
        removed_groups: before_groups - config.groups.len(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaudeHookError {
    InvalidInstallId,
    InvalidCommandToken { field: &'static str },
    InvalidMatcher,
    InvalidTimeout,
}

impl fmt::Display for ClaudeHookError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInstallId => write!(
                formatter,
                "install id must contain only ascii letters, digits, '-', or '_'"
            ),
            Self::InvalidCommandToken { field } => write!(
                formatter,
                "{field} must be non-empty and must not contain control characters"
            ),
            Self::InvalidMatcher => write!(
                formatter,
                "matcher must be non-empty and must not contain control characters"
            ),
            Self::InvalidTimeout => write!(formatter, "timeout must be greater than zero"),
        }
    }
}

impl std::error::Error for ClaudeHookError {}

fn tokmeter_hook_entry(settings: &TokmeterHookInstall, event: ClaudeHookEvent) -> ClaudeHookEntry {
    ClaudeHookEntry::command(format!(
        "{} hook --source claude-code --event {} --install-id {} # {}",
        settings.binary,
        event.as_str(),
        settings.install_id,
        settings.marker()
    ))
    .with_timeout_seconds(settings.timeout_seconds)
}

fn marker_for(install_id: &str) -> String {
    format!("{TOKMETER_MARKER_PREFIX}{install_id}")
}

fn validate_install_id(value: &str) -> Result<(), ClaudeHookError> {
    if value.is_empty()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(ClaudeHookError::InvalidInstallId);
    }

    Ok(())
}

fn validate_command_token(field: &'static str, value: &str) -> Result<(), ClaudeHookError> {
    if value.trim().is_empty()
        || value.contains('\0')
        || value.chars().any(|character| character.is_control())
    {
        return Err(ClaudeHookError::InvalidCommandToken { field });
    }

    Ok(())
}

fn validate_matcher(value: &str) -> Result<(), ClaudeHookError> {
    if value.trim().is_empty()
        || value.contains('\0')
        || value.chars().any(|character| character.is_control())
    {
        return Err(ClaudeHookError::InvalidMatcher);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> TokmeterHookInstall {
        TokmeterHookInstall::new("test-install", "/home/alice/.claude/settings.json").unwrap()
    }

    fn user_hook(command: &str) -> ClaudeHookEntry {
        ClaudeHookEntry::command(command).with_timeout_seconds(5)
    }

    #[test]
    fn detects_plausible_config_paths_from_home_dir() {
        let paths = plausible_config_paths("/home/alice");

        assert_eq!(paths[0], PathBuf::from("/home/alice/.claude/settings.json"));
        assert!(paths.contains(&PathBuf::from(
            "/home/alice/.config/claude-code/settings.json"
        )));
        assert!(paths.contains(&PathBuf::from("/home/alice/.claude.json")));
    }

    #[test]
    fn clean_install_adds_pre_and_post_tool_use_entries() {
        let mut config = ClaudeHooksConfig::default();
        let metadata = install_tokmeter_hooks(&mut config, &settings()).unwrap();

        assert_eq!(config.groups.len(), 2);
        assert_eq!(metadata.installed_entries.len(), 2);
        assert_eq!(metadata.created_groups.len(), 2);

        let events: Vec<ClaudeHookEvent> = config.groups.iter().map(|group| group.event).collect();
        assert_eq!(
            events,
            vec![ClaudeHookEvent::PreToolUse, ClaudeHookEvent::PostToolUse]
        );

        for group in &config.groups {
            assert_eq!(group.matcher, DEFAULT_MATCHER);
            assert_eq!(group.hooks.len(), 1);
            assert_eq!(group.hooks[0].kind, "command");
            assert_eq!(
                group.hooks[0].timeout_seconds,
                Some(DEFAULT_TIMEOUT_SECONDS)
            );
            assert!(
                group.hooks[0]
                    .command
                    .contains("vc-tokmeter hook --source claude-code")
            );
            assert!(
                group.hooks[0]
                    .command
                    .contains("vc-tokmeter-hook:test-install")
            );
        }
    }

    #[test]
    fn install_preserves_existing_config_entries() {
        let mut config = ClaudeHooksConfig::new(vec![
            ClaudeHookGroup::new(
                ClaudeHookEvent::PreToolUse,
                DEFAULT_MATCHER,
                vec![user_hook("echo user-pre")],
            ),
            ClaudeHookGroup::new(
                ClaudeHookEvent::PostToolUse,
                "Bash",
                vec![user_hook("echo user-post")],
            ),
        ]);

        let metadata = install_tokmeter_hooks(&mut config, &settings()).unwrap();

        let pre_group = config
            .groups
            .iter()
            .find(|group| group.event == ClaudeHookEvent::PreToolUse && group.matcher == "*")
            .unwrap();
        assert_eq!(pre_group.hooks.len(), 2);
        assert_eq!(pre_group.hooks[0].command, "echo user-pre");
        assert_eq!(
            pre_group.hooks[1].command,
            metadata.installed_entries[0].command
        );

        let bash_post_group = config
            .groups
            .iter()
            .find(|group| group.event == ClaudeHookEvent::PostToolUse && group.matcher == "Bash")
            .unwrap();
        assert_eq!(bash_post_group.hooks, vec![user_hook("echo user-post")]);
    }

    #[test]
    fn repeated_install_is_idempotent() {
        let mut config = ClaudeHooksConfig::default();
        let settings = settings();

        let first_metadata = install_tokmeter_hooks(&mut config, &settings).unwrap();
        let first_config = config.clone();
        let second_metadata = install_tokmeter_hooks(&mut config, &settings).unwrap();

        assert_eq!(config, first_config);
        assert_eq!(
            second_metadata.installed_entries,
            first_metadata.installed_entries
        );
        assert!(second_metadata.created_groups.is_empty());
        assert_eq!(
            config
                .groups
                .iter()
                .flat_map(|group| &group.hooks)
                .filter(|hook| hook.command.contains("vc-tokmeter-hook:test-install"))
                .count(),
            2
        );
    }

    #[test]
    fn uninstall_removes_only_tokmeter_created_entries() {
        let mut config = ClaudeHooksConfig::new(vec![ClaudeHookGroup::new(
            ClaudeHookEvent::PreToolUse,
            DEFAULT_MATCHER,
            vec![user_hook("echo keep-me")],
        )]);
        let metadata = install_tokmeter_hooks(&mut config, &settings()).unwrap();

        let report = uninstall_tokmeter_hooks(&mut config, &metadata);

        assert_eq!(
            report,
            HookUninstallReport {
                removed_entries: 2,
                removed_groups: 1,
            }
        );
        assert_eq!(config.groups.len(), 1);
        assert_eq!(config.groups[0].event, ClaudeHookEvent::PreToolUse);
        assert_eq!(config.groups[0].hooks, vec![user_hook("echo keep-me")]);
    }

    #[test]
    fn uninstall_does_not_remove_foreign_tokmeter_install() {
        let mut config = ClaudeHooksConfig::default();
        let metadata = install_tokmeter_hooks(&mut config, &settings()).unwrap();
        let other_settings =
            TokmeterHookInstall::new("other-install", "/home/alice/.claude/settings.json").unwrap();
        install_tokmeter_hooks(&mut config, &other_settings).unwrap();

        let report = uninstall_tokmeter_hooks(&mut config, &metadata);

        assert_eq!(report.removed_entries, 2);
        assert_eq!(
            config
                .groups
                .iter()
                .flat_map(|group| &group.hooks)
                .filter(|hook| hook.command.contains("vc-tokmeter-hook:other-install"))
                .count(),
            2
        );
    }
}
