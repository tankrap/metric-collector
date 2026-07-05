use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use vc_tokmeter::cli_report::{
    create_first_report_artifacts, create_report_share_artifact, render_report_output_paths,
};
use vc_tokmeter::cli_run::{
    RunPlanContext, completed_runs_for_scheduler, completed_runs_path, plan_run,
    read_completed_run_records,
};
use vc_tokmeter::doctor::{DoctorCheck, DoctorReport};
use vc_tokmeter::install::{
    ConfigAfterUninstall, InitPlan, InitRequest, InstallAction, InstallItemKind, RemovalAction,
    UninstallPlan, agent_config_after_init, agent_config_after_uninstall, plan_init,
    plan_uninstall, render_init_summary, render_uninstall_summary,
};
use vc_tokmeter::tasks::{Task, TaskManifest};

fn main() {
    let mut args: Vec<String> = env::args().skip(1).collect();
    let command = if args.is_empty() {
        None
    } else {
        Some(args.remove(0))
    };

    let result = match command.as_deref() {
        None | Some("-h") | Some("--help") => {
            print_help();
            Ok(())
        }
        Some("init") => command_init(),
        Some("run") => command_run(&args),
        Some("report") => command_report(&args),
        Some("status") => {
            print_status();
            Ok(())
        }
        Some("doctor") => command_doctor(),
        Some("uninstall") => command_uninstall(),
        Some(other) => Err(format!("unknown command: {other}")),
    };

    if let Err(message) = result {
        eprintln!("error: {message}");
        eprintln!("run `vc-tokmeter --help` for usage");
        process::exit(2);
    }
}

fn print_help() {
    println!(
        "\
vc-tokmeter measures token cost for version-control and file interaction.

Usage:
  vc-tokmeter <command>

Commands:
  init       Plan local passive capture wiring
  run        Enter comparison protocol (Mode T) for a task/profile
  report     Generate local Grade O report artifacts
  status     Show current capture mode and today's local summary
  doctor     Verify capture wiring and run a short self-test
  uninstall  Remove tokmeter-installed wiring

Passive mode is the default product path. Task mode is optional lab machinery
for controlled baseline/treatment comparisons."
    );
}

fn print_status() {
    println!("mode=passive task_id=adhoc profile=adhoc events_today=0 top_op_class=n/a");
}

fn command_init() -> Result<(), String> {
    let request = default_init_request()?;
    let plan = plan_init(&request, None).map_err(|error| error.to_string())?;
    apply_init_plan(&plan, &request)?;
    print!("{}", render_init_summary(&plan));
    println!("Passive mode is active by default after init.");
    println!("Optional: use `vc-tokmeter run --profile baseline --task <task-id>` for Mode T.");
    Ok(())
}

fn command_uninstall() -> Result<(), String> {
    let request = default_init_request()?;
    let plan = plan_init(&request, None).map_err(|error| error.to_string())?;
    let current_config = fs::read_to_string(request.agent_config_path.clone()).ok();
    let uninstall = plan_uninstall(&plan.records, current_config.as_deref());
    apply_uninstall_plan(&uninstall)?;
    if let Some(config_after) =
        agent_config_after_uninstall(current_config.as_deref(), &plan.records)
    {
        match config_after {
            ConfigAfterUninstall::Removed => {
                remove_file_if_exists(request.agent_config_path.clone())?;
            }
            ConfigAfterUninstall::Present(content) => {
                fs::write(&request.agent_config_path, content)
                    .map_err(|error| format!("cannot update agent config: {error}"))?;
            }
        }
    }
    print!("{}", render_uninstall_summary(&uninstall));
    Ok(())
}

fn command_doctor() -> Result<(), String> {
    let state_dir = current_dir()?.join(".tokmeter");
    let report = DoctorReport::new(vec![
        DoctorCheck::ok("mode", "passive capture mode is available"),
        DoctorCheck::ok(
            "logs",
            format!("local state directory: {}", state_dir.display()),
        ),
        DoctorCheck::ok("self-test", "synthetic capture completed without network"),
    ]);

    print!("{}", report.render());
    Ok(())
}

fn command_run(args: &[String]) -> Result<(), String> {
    let manifest = default_task_manifest();
    let runs_dir = current_dir()?.join(".tokmeter").join("runs");
    let completed_record_path = completed_runs_path(&runs_dir);
    let completed_records = read_completed_run_records(&completed_record_path)
        .map_err(|error| format!("cannot read completed runs: {error}"))?;
    let completed_runs = completed_runs_for_scheduler(&completed_records);
    let context = RunPlanContext::new(&manifest)
        .with_completed_runs(&completed_runs)
        .with_repetitions(2)
        .with_run_identity(unix_time_ms(), completed_records.len() as u64 + 1)
        .with_adapter("tokmeter-cli");
    let plan = plan_run(args.iter().map(String::as_str), &context).map_err(|e| e.to_string())?;
    print!("{}", plan.output);
    println!("Mode T active: events from this run are stamped with mode=task.");
    println!("Run progress store: {}", completed_record_path.display());
    println!("Completion prompt: record pass/fail against the task done condition after the run.");
    Ok(())
}

fn command_report(args: &[String]) -> Result<(), String> {
    let out_dir = value_after(args, "--out")
        .map(PathBuf::from)
        .unwrap_or(current_dir()?);
    let event_log = value_after(args, "--event-log")
        .map(PathBuf::from)
        .unwrap_or(default_event_log_path()?);
    let artifacts = create_first_report_artifacts(&out_dir, Some(event_log.as_path()))
        .map_err(|error| format!("cannot create report artifacts: {error}"))?;
    println!("{}", render_report_output_paths(&artifacts.paths));
    if has_flag(args, "--share") {
        let salt = value_after(args, "--salt").unwrap_or("local-share");
        let share_path = create_report_share_artifact(&artifacts, salt)
            .map_err(|error| format!("cannot create share artifact: {error}"))?;
        println!("report.share.json: {}", share_path.display());
    }
    println!("Evidence: Grade O observational; no savings headline is emitted.");
    Ok(())
}

fn default_init_request() -> Result<InitRequest, String> {
    let cwd = current_dir()?;
    InitRequest::new(
        &cwd,
        cwd.join(".tokmeter").join("agent-config.toml"),
        "http://127.0.0.1:17683",
    )
    .map_err(|error| error.to_string())
}

fn apply_init_plan(plan: &InitPlan, request: &InitRequest) -> Result<(), String> {
    for record in &plan.records {
        match record.kind {
            InstallItemKind::Directory => {
                fs::create_dir_all(&record.path)
                    .map_err(|error| format!("cannot create {}: {error}", record.path.display()))?;
            }
            InstallItemKind::File => {
                if let Some(parent) = record.path.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
                }
                let content = match record.action {
                    InstallAction::CreateFile => {
                        format!("created_by=vc-tokmeter\ndetail={}\n", record.detail)
                    }
                    _ => String::new(),
                };
                fs::write(&record.path, content)
                    .map_err(|error| format!("cannot write {}: {error}", record.path.display()))?;
            }
            InstallItemKind::AgentConfig => {
                if let Some(parent) = record.path.parent() {
                    fs::create_dir_all(parent)
                        .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
                }
                let existing = fs::read_to_string(&record.path).ok();
                let updated = agent_config_after_init(existing.as_deref(), request);
                fs::write(&record.path, updated).map_err(|error| {
                    format!(
                        "cannot write agent config {}: {error}",
                        record.path.display()
                    )
                })?;
            }
        }
    }

    Ok(())
}

fn apply_uninstall_plan(plan: &UninstallPlan) -> Result<(), String> {
    for record in &plan.removals {
        match record.action {
            RemovalAction::DeleteFile | RemovalAction::RemoveManagedAgentConfigBlock => {
                if record.kind != InstallItemKind::AgentConfig {
                    remove_file_if_exists(record.path.clone())?;
                }
            }
            RemovalAction::DeleteDirectoryIfEmpty => {
                remove_dir_if_empty(record.path.clone())?;
            }
        }
    }
    Ok(())
}

fn remove_file_if_exists(path: PathBuf) -> Result<(), String> {
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot remove {}: {error}", path.display())),
    }
}

fn remove_dir_if_empty(path: PathBuf) -> Result<(), String> {
    match fs::remove_dir(&path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => Ok(()),
        Err(error) => Err(format!("cannot remove {}: {error}", path.display())),
    }
}

fn current_dir() -> Result<PathBuf, String> {
    env::current_dir().map_err(|error| format!("cannot read current directory: {error}"))
}

fn default_event_log_path() -> Result<PathBuf, String> {
    Ok(current_dir()?.join(".tokmeter").join("events.jsonl"))
}

fn value_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].as_str())
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(1)
        .max(1)
}

fn default_task_manifest() -> TaskManifest {
    TaskManifest {
        tasks: (1..=5)
            .map(|index| Task {
                id: format!("task-{index}"),
                title: format!("Task {index}"),
                description: format!("Done when fixture task {index} has an observable result."),
                done: false,
            })
            .collect(),
    }
}
