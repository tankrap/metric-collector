use std::env;
use std::fs;
use std::io::{self, Read};
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{self, Command};
use std::thread;
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use vc_tokmeter::cli_report::{
    create_compare_report_artifacts, create_first_report_artifacts, create_report_share_artifact,
    render_report_output_paths,
};
use vc_tokmeter::cli_run::{
    RunPlanContext, WrappedRunOptions, completed_runs_for_scheduler, completed_runs_path,
    completion_decision_from_command_outcome, execute_process_command, execute_wrapped_run,
    plan_run, read_completed_run_records,
};
use vc_tokmeter::completion::{CompletionStatus, prompt_completion_status_from_stdin};
use vc_tokmeter::doctor::{DoctorCheck, DoctorReport};
use vc_tokmeter::hook_capture::{HookRuntimeRequest, execute_hook_runtime};
use vc_tokmeter::install::{
    ConfigAfterUninstall, InitPlan, InitRequest, InstallAction, InstallItemKind, RemovalAction,
    UninstallPlan, agent_config_after_init, agent_config_after_uninstall, plan_init,
    plan_uninstall, render_init_summary, render_uninstall_summary,
};
use vc_tokmeter::live_test::{
    DEFAULT_PROMPT, LiveTestRequest, LiveTestSurface, plan_live_test, render_live_test_plan,
    render_live_test_usage,
};
use vc_tokmeter::mcp_git::{McpGitConfig, run_mcp_git_server};
use vc_tokmeter::proxy::{ProxyConfig, run_proxy, serve_proxy_listener};
use vc_tokmeter::setup::{
    FsSetupEnvironment, SetupRequest, UploadSetup, detect_surfaces, plan_setup, render_setup_plan,
};
use vc_tokmeter::structured_import::{
    StructuredUsageDefaults, append_codex_exec_jsonl_to_event_log,
};
use vc_tokmeter::tasks::{Task, TaskManifest};
use vc_tokmeter::upload::{
    UploadPlanRequest, default_upload_config_path, prepare_upload_plan, remove_upload_config,
    render_upload_plan, render_upload_response, send_upload, write_upload_config,
};

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
        Some("setup") => command_setup(&args),
        Some("hook") => command_hook(&args),
        Some("import-usage") => command_import_usage(&args),
        Some("mcp-git") => command_mcp_git(&args),
        Some("proxy") => command_proxy(&args),
        Some("codex-tui") => command_codex_tui(&args),
        Some("claude-code") => command_claude_code(&args),
        Some("live-test") => command_live_test(&args),
        Some("run") => command_run(&args),
        Some("report") => command_report(&args),
        Some("upload") => command_upload(&args),
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
  setup      Configure a repo and print next commands for external testers
  hook       Execute a local agent hook payload from stdin
  import-usage
             Import structured usage JSONL into the event log
  mcp-git    Run stdio MCP git tools for desktop apps
  proxy      Run the localhost-only provider proxy
  codex-tui  Launch interactive Codex through the local proxy
  claude-code Launch interactive Claude Code through the local proxy
  live-test  Print live test commands for Codex, Claude Code, and Claude Desktop
  run        Enter comparison protocol (Mode T) for a task/profile
  report     Generate local Grade O report artifacts
  upload     Prepare or send an opt-in redacted metrics upload
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

fn command_setup(args: &[String]) -> Result<(), String> {
    if has_flag(args, "-h") || has_flag(args, "--help") {
        print_setup_usage();
        return Ok(());
    }

    let mut repo = current_dir()?;
    let mut tokmeter_bin = tokmeter_binary_command();
    let mut upload = UploadSetup::LocalOnly;
    let mut upload_endpoint: Option<String> = None;
    let mut upload_token: Option<String> = None;
    let mut remove_saved_upload_config = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--repo" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--repo requires a path".to_owned())?;
                repo = PathBuf::from(value);
            }
            "--tokmeter-bin" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--tokmeter-bin requires a command".to_owned())?;
                tokmeter_bin = value.clone();
            }
            "--local-only" => {
                upload = UploadSetup::LocalOnly;
            }
            "--upload-endpoint" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--upload-endpoint requires a URL".to_owned())?;
                upload_endpoint = Some(value.clone());
            }
            "--upload-token" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--upload-token requires a token".to_owned())?;
                upload_token = Some(value.clone());
            }
            "--remove-upload-config" => {
                remove_saved_upload_config = true;
            }
            other => return Err(format!("unknown setup argument: {other}")),
        }
        index += 1;
    }

    if !repo.is_dir() {
        return Err(format!("repo does not exist: {}", repo.display()));
    }

    if remove_saved_upload_config {
        if upload_endpoint.is_some() || upload_token.is_some() {
            return Err(
                "--remove-upload-config cannot be combined with upload enrollment flags".to_owned(),
            );
        }
        let config_path = upload_config_path_for_repo(&repo);
        let removed = remove_upload_config(&config_path)
            .map_err(|error| format!("cannot remove upload config: {error}"))?;
        if removed {
            println!("upload config removed: {}", config_path.display());
        } else {
            println!("upload config already absent: {}", config_path.display());
        }
        return Ok(());
    }

    let upload_config_to_save = match (upload_endpoint, upload_token) {
        (Some(endpoint), Some(token)) => {
            upload = UploadSetup::OptIn {
                endpoint: endpoint.clone(),
            };
            Some((endpoint, token))
        }
        (Some(_), None) => return Err("--upload-endpoint requires --upload-token".to_owned()),
        (None, Some(_)) => return Err("--upload-token requires --upload-endpoint".to_owned()),
        (None, None) => None,
    };

    let init_request = init_request_for_repo(repo.clone())?;
    let init_plan = plan_init(&init_request, None).map_err(|error| error.to_string())?;
    apply_init_plan(&init_plan, &init_request)?;
    if let Some((endpoint, token)) = upload_config_to_save.as_ref() {
        write_upload_config(&upload_config_path_for_repo(&repo), endpoint, token)
            .map_err(|error| format!("cannot save upload config: {error}"))?;
    }

    let setup_request = SetupRequest {
        repo_root: repo,
        tokmeter_bin,
        upload,
        detection: detect_surfaces(&FsSetupEnvironment),
    };
    let setup_plan = plan_setup(&setup_request);
    print!("{}", render_init_summary(&init_plan));
    print!("{}", render_setup_plan(&setup_plan));
    println!("Verification:");
    println!("  - {} doctor", setup_request.tokmeter_bin);
    if matches!(setup_request.upload, UploadSetup::OptIn { .. }) {
        println!("  - {} upload --dry-run", setup_request.tokmeter_bin);
        println!("  - {} upload --yes", setup_request.tokmeter_bin);
    }
    Ok(())
}

fn print_setup_usage() {
    println!(
        "\
Usage:
  vc-tokmeter setup [--repo PATH] [--local-only]
  vc-tokmeter setup [--repo PATH] --upload-endpoint URL --upload-token TOKEN
  vc-tokmeter setup [--repo PATH] --remove-upload-config

Options:
  --repo PATH          Target repository to configure (default: current directory)
  --tokmeter-bin CMD   Command printed in generated next steps
  --local-only         Keep all reporting local (default)
  --upload-endpoint URL
                       Save opt-in collector endpoint with --upload-token
  --upload-token TOKEN Save opt-in upload token; value is not printed
  --remove-upload-config
                       Remove saved opt-in upload endpoint and token"
    );
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
    let (run_args, command_args) = split_wrapped_command(args);
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

    if let Some((program, program_args)) = command_args.split_first() {
        let done_condition =
            "Done when the wrapped command exits and the task has an observable result.";
        let options = WrappedRunOptions::new(
            &completed_record_path,
            done_condition,
            unix_time_ms(),
            unix_time_seconds(),
        )
        .with_completion_fallback(CompletionStatus::Aborted);
        let result = execute_wrapped_run(
            run_args.iter().map(String::as_str),
            &context,
            &options,
            |_plan| execute_process_command(program, program_args.iter().cloned()),
            |_plan, command| {
                let fallback = completion_decision_from_command_outcome(command).status;
                prompt_completion_status_from_stdin(done_condition, fallback)
                    .unwrap_or_else(|_| completion_decision_from_command_outcome(command))
            },
        )
        .map_err(|error| error.to_string())?;

        print!("{}", result.plan.output);
        println!(
            "Wrapped command exited: succeeded={} exit_code={}",
            result.command.succeeded,
            result
                .command
                .exit_code
                .map(|code| code.to_string())
                .unwrap_or_else(|| "signal".to_owned())
        );
        println!("Completion status: {}", result.completion.status);
        println!("Run progress store: {}", completed_record_path.display());
        return Ok(());
    }

    let plan =
        plan_run(run_args.iter().map(String::as_str), &context).map_err(|e| e.to_string())?;
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
    let artifacts = if has_flag(args, "--compare") {
        let completed_runs = value_after(args, "--completed-runs")
            .map(PathBuf::from)
            .unwrap_or(default_completed_runs_path()?);
        create_compare_report_artifacts(&out_dir, event_log.as_path(), completed_runs.as_path())
            .map_err(|error| format!("cannot create compare report artifacts: {error}"))?
    } else {
        create_first_report_artifacts(&out_dir, Some(event_log.as_path()))
            .map_err(|error| format!("cannot create report artifacts: {error}"))?
    };
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

fn command_upload(args: &[String]) -> Result<(), String> {
    let event_log = value_after(args, "--event-log")
        .map(PathBuf::from)
        .unwrap_or(default_event_log_path()?);
    let out_dir = value_after(args, "--out")
        .map(PathBuf::from)
        .unwrap_or(current_dir()?.join(".tokmeter").join("report"));
    let dry_run = upload_should_dry_run(args);
    let request = UploadPlanRequest {
        event_log_path: event_log,
        out_dir,
        endpoint: value_after(args, "--endpoint").map(str::to_owned),
        token: value_after(args, "--token").map(str::to_owned),
        config_path: value_after(args, "--config").map(PathBuf::from),
        dry_run,
        yes: has_flag(args, "--yes"),
    };
    let plan =
        prepare_upload_plan(&request).map_err(|error| format!("cannot prepare upload: {error}"))?;
    print!("{}", render_upload_plan(&plan));
    if !plan.dry_run {
        let upload_request = plan
            .request
            .as_ref()
            .ok_or_else(|| "cannot upload without endpoint and token".to_owned())?;
        let response = send_upload(upload_request)
            .map_err(|error| format!("cannot upload metrics payload: {error}"))?;
        print!("{}", render_upload_response(&response));
    }
    Ok(())
}

fn command_hook(args: &[String]) -> Result<(), String> {
    let event_log = value_after(args, "--event-log")
        .map(PathBuf::from)
        .unwrap_or(default_event_log_path()?);
    let source = value_after(args, "--source").unwrap_or("claude-code");
    let mut stdin_payload = String::new();
    io::stdin()
        .read_to_string(&mut stdin_payload)
        .map_err(|error| format!("cannot read hook payload from stdin: {error}"))?;
    let request = HookRuntimeRequest::new(
        event_log.clone(),
        stdin_payload,
        unix_time_ms(),
        format!("hook-{}", unix_time_ms()),
    )
    .with_source(source);
    let executed = execute_hook_runtime(&request)
        .map_err(|error| format!("cannot execute hook payload: {error}"))?;
    println!(
        "hook events appended: {} path={}",
        executed.captured.events.len(),
        event_log.display()
    );
    Ok(())
}

fn command_import_usage(args: &[String]) -> Result<(), String> {
    let event_log = value_after(args, "--event-log")
        .map(PathBuf::from)
        .unwrap_or(default_event_log_path()?);
    let mut input = String::new();
    if let Some(source) = value_after(args, "--source") {
        input = fs::read_to_string(source)
            .map_err(|error| format!("cannot read usage source {source}: {error}"))?;
    } else {
        io::stdin()
            .read_to_string(&mut input)
            .map_err(|error| format!("cannot read usage JSONL from stdin: {error}"))?;
    }

    let defaults = StructuredUsageDefaults {
        timestamp_ms: unix_time_ms(),
        run_id: value_after(args, "--run-id")
            .unwrap_or("imported-codex-exec")
            .to_owned(),
        task_id: value_after(args, "--task-id").unwrap_or("adhoc").to_owned(),
        profile_id: value_after(args, "--profile-id")
            .unwrap_or("adhoc")
            .to_owned(),
        adapter: value_after(args, "--adapter")
            .unwrap_or("import.codex.exec")
            .to_owned(),
    };

    let summary = append_codex_exec_jsonl_to_event_log(&input, &event_log, &defaults)
        .map_err(|error| format!("cannot import structured usage: {error}"))?;

    println!(
        "usage events imported: {} skipped_duplicates={} diagnostics={} event_log={}",
        summary.imported,
        summary.skipped_duplicates,
        summary.diagnostics.len(),
        event_log.display()
    );
    for diagnostic in summary.diagnostics.iter().take(5) {
        println!(
            "diagnostic line={} kind={}",
            diagnostic.line, diagnostic.kind
        );
    }
    if summary.diagnostics.len() > 5 {
        println!(
            "diagnostics truncated: {} more",
            summary.diagnostics.len().saturating_sub(5)
        );
    }

    Ok(())
}

fn command_mcp_git(args: &[String]) -> Result<(), String> {
    let event_log = value_after(args, "--event-log")
        .map(PathBuf::from)
        .unwrap_or(default_event_log_path()?);
    let workdir = value_after(args, "--workdir")
        .map(PathBuf::from)
        .unwrap_or(current_dir()?);
    let run_id = value_after(args, "--run-id")
        .map(str::to_owned)
        .unwrap_or_else(|| format!("mcp-git-{}", unix_time_ms()));
    let config = McpGitConfig {
        event_log_path: event_log,
        workdir,
        run_id,
    };
    let stdin = io::stdin();
    let stdout = io::stdout();
    run_mcp_git_server(stdin.lock(), stdout.lock(), config)
        .map_err(|error| format!("mcp-git failed: {error}"))
}

fn command_proxy(args: &[String]) -> Result<(), String> {
    let bind_host = value_after(args, "--bind-host").unwrap_or("127.0.0.1");
    let bind_port = value_after(args, "--port")
        .unwrap_or("17683")
        .parse::<u16>()
        .map_err(|error| format!("invalid --port: {error}"))?;
    let upstream = value_after(args, "--upstream")
        .ok_or_else(|| "--upstream is required for proxy".to_owned())?;
    let event_log = value_after(args, "--event-log")
        .map(PathBuf::from)
        .unwrap_or(default_event_log_path()?);
    let config = ProxyConfig::new(bind_host, bind_port, upstream)
        .map_err(|e| e.to_string())?
        .with_event_log_path(event_log.clone());
    println!(
        "proxy listening on {}:{} -> {}",
        config.bind_host, config.bind_port, config.upstream_url
    );
    println!("proxy event log: {}", event_log.display());
    run_proxy(config).map_err(|error| format!("proxy failed: {error}"))
}

fn command_codex_tui(args: &[String]) -> Result<(), String> {
    let (session_args, codex_args) = split_passthrough_args(args);
    let provider = codex_provider(session_args)?;
    let bind_host = value_after(session_args, "--bind-host").unwrap_or("127.0.0.1");
    let bind_port = proxy_wrapper_bind_port(session_args)?;
    let upstream = value_after(session_args, "--upstream").unwrap_or_else(|| provider.upstream());
    let event_log = value_after(session_args, "--event-log")
        .map(PathBuf::from)
        .unwrap_or(default_event_log_path()?);
    let report_out = value_after(session_args, "--out")
        .map(PathBuf::from)
        .unwrap_or(current_dir()?.join(".tokmeter").join("report"));
    let codex_bin = value_after(session_args, "--codex-bin").unwrap_or("codex");
    let keep_openai_api_key = has_flag(session_args, "--keep-openai-api-key");

    if let Some(parent) = event_log.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }

    let mut config = ProxyConfig::new(bind_host, bind_port, upstream)
        .map_err(|error| error.to_string())?
        .with_event_log_path(event_log.clone());
    let listener = TcpListener::bind((config.bind_host.as_str(), config.bind_port))
        .map_err(|error| format!("cannot bind proxy: {error}"))?;
    config.bind_port = listener
        .local_addr()
        .map_err(|error| format!("cannot read proxy listener address: {error}"))?
        .port();
    let base_override = provider.base_override(&config.bind_host, config.bind_port);
    let proxy_config = config.clone();

    println!(
        "tokmeter proxy listening on {}:{} -> {}",
        config.bind_host, config.bind_port, config.upstream_url
    );
    println!("tokmeter event log: {}", event_log.display());
    println!(
        "codex config override: {}=\"{}\"",
        base_override.key, base_override.value
    );
    println!(
        "report while Codex is running: vc-tokmeter report --event-log {} --out {}",
        event_log.display(),
        report_out.display()
    );

    thread::spawn(move || {
        if let Err(error) = serve_proxy_listener(proxy_config, listener) {
            eprintln!("tokmeter proxy failed: {error}");
        }
    });
    thread::sleep(Duration::from_millis(150));

    let mut command = Command::new(codex_bin);
    command
        .arg("-c")
        .arg(format!("{}=\"{}\"", base_override.key, base_override.value));
    command.args(codex_args);
    if provider == CodexProvider::ChatGpt && !keep_openai_api_key {
        command.env_remove("OPENAI_API_KEY");
    }

    let status = command
        .status()
        .map_err(|error| format!("cannot launch {codex_bin}: {error}"))?;

    match create_first_report_artifacts(&report_out, Some(event_log.as_path())) {
        Ok(artifacts) => {
            println!("{}", render_report_output_paths(&artifacts.paths));
            println!("Evidence: Grade O observational; no savings headline is emitted.");
        }
        Err(error) => eprintln!("cannot create report artifacts: {error}"),
    }

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "codex exited with {}",
            status
                .code()
                .map(|code| format!("code {code}"))
                .unwrap_or_else(|| "signal".to_owned())
        ))
    }
}

fn command_claude_code(args: &[String]) -> Result<(), String> {
    let (session_args, claude_args) = split_passthrough_args(args);
    let bind_host = value_after(session_args, "--bind-host").unwrap_or("127.0.0.1");
    let bind_port = proxy_wrapper_bind_port(session_args)?;
    let upstream = value_after(session_args, "--upstream").unwrap_or("https://api.anthropic.com");
    let event_log = value_after(session_args, "--event-log")
        .map(PathBuf::from)
        .unwrap_or(default_event_log_path()?);
    let report_out = value_after(session_args, "--out")
        .map(PathBuf::from)
        .unwrap_or(current_dir()?.join(".tokmeter").join("report"));
    let claude_bin = value_after(session_args, "--claude-bin").unwrap_or("claude");
    let keep_anthropic_api_key = has_flag(session_args, "--keep-anthropic-api-key");

    if let Some(parent) = event_log.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }

    let mut config = ProxyConfig::new(bind_host, bind_port, upstream)
        .map_err(|error| error.to_string())?
        .with_event_log_path(event_log.clone())
        .with_adapter_label("proxy.claude.anthropic");
    let listener = TcpListener::bind((config.bind_host.as_str(), config.bind_port))
        .map_err(|error| format!("cannot bind proxy: {error}"))?;
    config.bind_port = listener
        .local_addr()
        .map_err(|error| format!("cannot read proxy listener address: {error}"))?
        .port();
    let anthropic_base_url = claude_anthropic_base_url(&config.bind_host, config.bind_port);
    let proxy_config = config.clone();

    println!(
        "tokmeter proxy listening on {}:{} -> {}",
        config.bind_host, config.bind_port, config.upstream_url
    );
    println!("tokmeter event log: {}", event_log.display());
    println!("claude env override: ANTHROPIC_BASE_URL=\"{anthropic_base_url}\"");
    println!(
        "report while Claude Code is running: vc-tokmeter report --event-log {} --out {}",
        event_log.display(),
        report_out.display()
    );

    thread::spawn(move || {
        if let Err(error) = serve_proxy_listener(proxy_config, listener) {
            eprintln!("tokmeter proxy failed: {error}");
        }
    });
    thread::sleep(Duration::from_millis(150));

    let mut command = Command::new(claude_bin);
    command.env("ANTHROPIC_BASE_URL", &anthropic_base_url);
    command.args(claude_args);
    if !keep_anthropic_api_key {
        command.env_remove("ANTHROPIC_API_KEY");
    }

    let status = command
        .status()
        .map_err(|error| format!("cannot launch {claude_bin}: {error}"))?;

    match create_first_report_artifacts(&report_out, Some(event_log.as_path())) {
        Ok(artifacts) => {
            println!("{}", render_report_output_paths(&artifacts.paths));
            println!("Evidence: Grade O observational; no savings headline is emitted.");
        }
        Err(error) => eprintln!("cannot create report artifacts: {error}"),
    }

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "claude exited with {}",
            status
                .code()
                .map(|code| format!("code {code}"))
                .unwrap_or_else(|| "signal".to_owned())
        ))
    }
}

fn command_live_test(args: &[String]) -> Result<(), String> {
    if args.is_empty() || has_flag(args, "-h") || has_flag(args, "--help") {
        print!("{}", render_live_test_usage());
        return Ok(());
    }

    let surface = args[0].parse::<LiveTestSurface>()?;
    let mut repo = current_dir()?;
    let mut prompt = DEFAULT_PROMPT.to_owned();
    let mut tokmeter_bin = tokmeter_binary_command();
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--repo" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--repo requires a path".to_owned())?;
                repo = PathBuf::from(value);
            }
            "--prompt" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--prompt requires text".to_owned())?;
                prompt = value.clone();
            }
            "--tokmeter-bin" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| "--tokmeter-bin requires a command".to_owned())?;
                tokmeter_bin = value.clone();
            }
            other => return Err(format!("unknown live-test argument: {other}")),
        }
        index += 1;
    }

    if !repo.is_dir() {
        return Err(format!("repo does not exist: {}", repo.display()));
    }

    let request = LiveTestRequest::new(surface, repo)
        .with_prompt(prompt)
        .with_tokmeter_bin(tokmeter_bin);
    let plan = plan_live_test(&request);
    print!("{}", render_live_test_plan(&plan));
    Ok(())
}

fn default_init_request() -> Result<InitRequest, String> {
    init_request_for_repo(current_dir()?)
}

fn init_request_for_repo(repo: PathBuf) -> Result<InitRequest, String> {
    InitRequest::new(
        &repo,
        repo.join(".tokmeter").join("agent-config.toml"),
        "http://127.0.0.1:17683",
    )
    .map_err(|error| error.to_string())
}

fn upload_config_path_for_repo(repo: &std::path::Path) -> PathBuf {
    default_upload_config_path(&repo.join(".tokmeter").join("events.jsonl"))
}

fn apply_init_plan(plan: &InitPlan, request: &InitRequest) -> Result<(), String> {
    let event_log_path = request.install_root.join(".tokmeter").join("events.jsonl");
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
                    InstallAction::CreateFile
                        if record.path.file_name().and_then(|name| name.to_str())
                            == Some("claude-code-hook.json") =>
                    {
                        claude_hook_file_content(&event_log_path)
                    }
                    InstallAction::CreateFile
                        if record.path.file_name().and_then(|name| name.to_str())
                            == Some("hooks.json")
                            && record
                                .path
                                .parent()
                                .and_then(|path| path.file_name())
                                .and_then(|name| name.to_str())
                                == Some(".codex") =>
                    {
                        if record.path.exists() {
                            let existing = fs::read_to_string(&record.path).map_err(|error| {
                                format!(
                                    "cannot inspect existing Codex hooks {}: {error}",
                                    record.path.display()
                                )
                            })?;
                            if !existing.contains("vc-tokmeter hook --source codex") {
                                return Err(format!(
                                    "{} already exists; move it aside or merge tokmeter hooks manually before running init",
                                    record.path.display()
                                ));
                            }
                            existing
                        } else {
                            codex_hook_file_content(&event_log_path)
                        }
                    }
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CodexProvider {
    ChatGpt,
    Api,
}

impl CodexProvider {
    fn upstream(self) -> &'static str {
        match self {
            Self::ChatGpt => "https://chatgpt.com/backend-api",
            Self::Api => "https://api.openai.com",
        }
    }

    fn base_override(self, bind_host: &str, bind_port: u16) -> CodexBaseOverride {
        match self {
            Self::ChatGpt => CodexBaseOverride {
                key: "chatgpt_base_url",
                value: format!("http://{bind_host}:{bind_port}/backend-api/"),
            },
            Self::Api => CodexBaseOverride {
                key: "openai_base_url",
                value: format!("http://{bind_host}:{bind_port}/v1"),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct CodexBaseOverride {
    key: &'static str,
    value: String,
}

fn codex_provider(args: &[String]) -> Result<CodexProvider, String> {
    match value_after(args, "--provider").unwrap_or("chatgpt") {
        "chatgpt" | "subscription" => Ok(CodexProvider::ChatGpt),
        "api" | "platform" => Ok(CodexProvider::Api),
        value => Err(format!(
            "invalid --provider: {value}; expected chatgpt or api"
        )),
    }
}

fn split_passthrough_args(args: &[String]) -> (&[String], &[String]) {
    match args.iter().position(|arg| arg == "--") {
        Some(index) => (&args[..index], &args[index + 1..]),
        None => (args, &[]),
    }
}

fn proxy_wrapper_bind_port(args: &[String]) -> Result<u16, String> {
    value_after(args, "--port")
        .unwrap_or("0")
        .parse::<u16>()
        .map_err(|error| format!("invalid --port: {error}"))
}

fn claude_anthropic_base_url(bind_host: &str, bind_port: u16) -> String {
    format!("http://{bind_host}:{bind_port}")
}

fn default_event_log_path() -> Result<PathBuf, String> {
    Ok(current_dir()?.join(".tokmeter").join("events.jsonl"))
}

fn default_completed_runs_path() -> Result<PathBuf, String> {
    Ok(completed_runs_path(
        current_dir()?.join(".tokmeter").join("runs"),
    ))
}

fn claude_hook_file_content(event_log_path: &std::path::Path) -> String {
    let command = shell_quote(&tokmeter_binary_command());
    let pre_command = json_string(&format!(
        "{command} hook --source claude-code --event PreToolUse --event-log {}",
        event_log_path.display()
    ));
    let post_command = json_string(&format!(
        "{command} hook --source claude-code --event PostToolUse --event-log {}",
        event_log_path.display()
    ));

    format!(
        "{{\n  \"hooks\": [\n    {{\n      \"event\": \"PreToolUse\",\n      \"command\": {}\n    }},\n    {{\n      \"event\": \"PostToolUse\",\n      \"command\": {}\n    }}\n  ]\n}}\n",
        pre_command, post_command
    )
}

fn codex_hook_file_content(event_log_path: &std::path::Path) -> String {
    let command = shell_quote(&tokmeter_binary_command());
    let pre_command = json_string(&format!(
        "{command} hook --source codex --event PreToolUse --event-log {}",
        event_log_path.display()
    ));
    let post_command = json_string(&format!(
        "{command} hook --source codex --event PostToolUse --event-log {}",
        event_log_path.display()
    ));

    format!(
        concat!(
            "{{\n",
            "  \"hooks\": {{\n",
            "    \"PreToolUse\": [\n",
            "      {{\n",
            "        \"matcher\": \"*\",\n",
            "        \"hooks\": [\n",
            "          {{\n",
            "            \"type\": \"command\",\n",
            "            \"command\": {},\n",
            "            \"timeout\": 30,\n",
            "            \"statusMessage\": \"Recording tokmeter tool input\"\n",
            "          }}\n",
            "        ]\n",
            "      }}\n",
            "    ],\n",
            "    \"PostToolUse\": [\n",
            "      {{\n",
            "        \"matcher\": \"*\",\n",
            "        \"hooks\": [\n",
            "          {{\n",
            "            \"type\": \"command\",\n",
            "            \"command\": {},\n",
            "            \"timeout\": 30,\n",
            "            \"statusMessage\": \"Recording tokmeter tool output\"\n",
            "          }}\n",
            "        ]\n",
            "      }}\n",
            "    ]\n",
            "  }}\n",
            "}}\n"
        ),
        pre_command, post_command
    )
}

fn tokmeter_binary_command() -> String {
    env::current_exe()
        .ok()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "vc-tokmeter".to_owned())
}

fn shell_quote(value: &str) -> String {
    if value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-'))
    {
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

fn value_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|window| window[0] == flag)
        .map(|window| window[1].as_str())
}

fn has_flag(args: &[String], flag: &str) -> bool {
    args.iter().any(|arg| arg == flag)
}

fn upload_should_dry_run(args: &[String]) -> bool {
    has_flag(args, "--dry-run") || !has_flag(args, "--yes")
}

fn split_wrapped_command(args: &[String]) -> (&[String], &[String]) {
    args.iter()
        .position(|arg| arg == "--")
        .map(|index| (&args[..index], &args[index + 1..]))
        .unwrap_or((args, &[]))
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(1)
        .max(1)
}

fn unix_time_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn temp_repo(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("vc-tokmeter-main-{name}-{nanos}"));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn setup_defaults_to_local_only_without_upload_config() {
        let repo = temp_repo("local-only");

        command_setup(&args(&["--repo", repo.to_str().unwrap()])).unwrap();

        assert!(!repo.join(".tokmeter").join("upload.json").exists());
    }

    #[test]
    fn setup_requires_explicit_endpoint_and_token_for_upload_config() {
        let repo = temp_repo("upload-requires-pair");

        let endpoint_only = command_setup(&args(&[
            "--repo",
            repo.to_str().unwrap(),
            "--upload-endpoint",
            "https://collector.example.test/upload",
        ]))
        .unwrap_err();
        let token_only = command_setup(&args(&[
            "--repo",
            repo.to_str().unwrap(),
            "--upload-token",
            "secret-token",
        ]))
        .unwrap_err();

        assert!(endpoint_only.contains("--upload-endpoint requires --upload-token"));
        assert!(token_only.contains("--upload-token requires --upload-endpoint"));
        assert!(!repo.join(".tokmeter").join("upload.json").exists());
    }

    #[test]
    fn setup_saves_and_removes_opt_in_upload_config() {
        let repo = temp_repo("upload-config");
        let config = repo.join(".tokmeter").join("upload.json");

        command_setup(&args(&[
            "--repo",
            repo.to_str().unwrap(),
            "--upload-endpoint",
            "https://collector.example.test/upload",
            "--upload-token",
            "saved-secret-token",
        ]))
        .unwrap();
        let saved = fs::read_to_string(&config).unwrap();

        assert!(saved.contains("\"enabled\": true"));
        assert!(saved.contains("\"endpoint\": \"https://collector.example.test/upload\""));
        assert!(saved.contains("\"upload_token\": \"saved-secret-token\""));

        command_setup(&args(&[
            "--repo",
            repo.to_str().unwrap(),
            "--remove-upload-config",
        ]))
        .unwrap();

        assert!(!config.exists());
    }

    #[test]
    fn upload_command_defaults_to_dry_run_until_yes_is_present() {
        assert!(upload_should_dry_run(&args(&[])));
        assert!(upload_should_dry_run(&args(&[
            "--endpoint",
            "https://example.test"
        ])));
        assert!(upload_should_dry_run(&args(&["--dry-run", "--yes"])));
        assert!(!upload_should_dry_run(&args(&["--yes"])));
    }

    #[test]
    fn codex_tui_defaults_to_chatgpt_subscription_proxy() {
        let provider = codex_provider(&[]).unwrap();
        let base = provider.base_override("127.0.0.1", 17683);

        assert_eq!(provider.upstream(), "https://chatgpt.com/backend-api");
        assert_eq!(base.key, "chatgpt_base_url");
        assert_eq!(base.value, "http://127.0.0.1:17683/backend-api/");
    }

    #[test]
    fn codex_tui_supports_platform_api_proxy() {
        let provider = codex_provider(&args(&["--provider", "api"])).unwrap();
        let base = provider.base_override("localhost", 48123);

        assert_eq!(provider.upstream(), "https://api.openai.com");
        assert_eq!(base.key, "openai_base_url");
        assert_eq!(base.value, "http://localhost:48123/v1");
    }

    #[test]
    fn proxy_wrappers_default_to_os_assigned_ports() {
        assert_eq!(proxy_wrapper_bind_port(&args(&[])).unwrap(), 0);
        assert_eq!(
            proxy_wrapper_bind_port(&args(&["--port", "17683"])).unwrap(),
            17683
        );
    }

    #[test]
    fn codex_tui_splits_passthrough_args_after_double_dash() {
        let input = args(&[
            "--event-log",
            ".tokmeter/events.jsonl",
            "--",
            "--model",
            "gpt-5.5",
            "initial prompt",
        ]);
        let (session_args, codex_args) = split_passthrough_args(&input);

        assert_eq!(session_args, &input[..2]);
        assert_eq!(codex_args, &input[3..]);
    }

    #[test]
    fn claude_code_uses_anthropic_base_url_without_api_path_suffix() {
        assert_eq!(
            claude_anthropic_base_url("127.0.0.1", 17684),
            "http://127.0.0.1:17684"
        );
    }

    #[test]
    fn claude_code_splits_passthrough_args_after_double_dash() {
        let input = args(&[
            "--event-log",
            ".tokmeter/events.jsonl",
            "--",
            "--model",
            "sonnet",
            "start from git status",
        ]);
        let (session_args, claude_args) = split_passthrough_args(&input);

        assert_eq!(session_args, &input[..2]);
        assert_eq!(claude_args, &input[3..]);
    }
}
