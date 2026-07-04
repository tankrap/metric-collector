use std::env;
use std::process;

fn main() {
    let mut args = env::args().skip(1);
    let command = args.next();

    let result = match command.as_deref() {
        None | Some("-h") | Some("--help") => {
            print_help();
            Ok(())
        }
        Some("init") => not_implemented("init"),
        Some("run") => not_implemented("run"),
        Some("report") => not_implemented("report"),
        Some("status") => {
            print_status();
            Ok(())
        }
        Some("doctor") => not_implemented("doctor"),
        Some("uninstall") => not_implemented("uninstall"),
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
  init       Detect agent tooling and install local capture wiring
  run        Wrap or stamp an agent session for a task/profile
  report     Generate local report artifacts
  status     Show current capture mode and today's local summary
  doctor     Verify capture wiring and run a short self-test
  uninstall  Remove tokmeter-installed wiring

This early implementation has module scaffolding only; command behavior is
being built behind these stable names."
    );
}

fn not_implemented(command: &str) -> Result<(), String> {
    Err(format!("{command} is not implemented yet"))
}

fn print_status() {
    println!("mode=passive task_id=adhoc profile=adhoc events_today=0 top_op_class=n/a");
}
