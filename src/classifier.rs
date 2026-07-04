pub const GIT_STATUS: &str = "git_status";
pub const GIT_DIFF: &str = "git_diff";
pub const GIT_LOG: &str = "git_log";
pub const GIT_SHOW: &str = "git_show";
pub const GIT_BRANCH: &str = "git_branch";
pub const GIT_PUSH: &str = "git_push";
pub const GIT_PULL: &str = "git_pull";
pub const FILE_READ: &str = "file_read";
pub const FILE_SEARCH: &str = "file_search";
pub const FILE_LIST: &str = "file_list";
pub const TEST_OUTPUT: &str = "test_output";
pub const BUILD_OUTPUT: &str = "build_output";
pub const EDIT_ECHO: &str = "edit_echo";
pub const OTHER: &str = "other";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Classification {
    pub operation_class: &'static str,
    pub generated_file: bool,
}

impl Classification {
    pub const fn new(operation_class: &'static str, generated_file: bool) -> Self {
        Self {
            operation_class,
            generated_file,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Event<'a> {
    pub tool_name: Option<&'a str>,
    pub command: Option<&'a str>,
    pub path: Option<&'a str>,
    pub output: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct Classifier {
    rules: Vec<Rule>,
}

impl Default for Classifier {
    fn default() -> Self {
        Self::new()
    }
}

impl Classifier {
    pub fn new() -> Self {
        Self {
            rules: default_rules(),
        }
    }

    pub fn classify(&self, event: &Event<'_>) -> Classification {
        let generated_file = event
            .path
            .into_iter()
            .chain(event.command)
            .chain(event.output)
            .any(is_generated_path);

        for rule in &self.rules {
            if rule.matches(event) {
                return Classification::new(rule.operation_class, generated_file);
            }
        }

        Classification::new(OTHER, generated_file)
    }

    pub fn classify_command(&self, command: &str) -> Classification {
        self.classify(&Event {
            command: Some(command),
            ..Event::default()
        })
    }

    pub fn classify_tool_use(&self, tool_name: &str, input: &str) -> Classification {
        let is_bash = eq_ignore_ascii_case(tool_name, "bash");
        self.classify(&Event {
            tool_name: Some(tool_name),
            command: is_bash.then_some(input),
            path: (!is_bash).then_some(input),
            ..Event::default()
        })
    }
}

#[derive(Debug, Clone)]
struct Rule {
    operation_class: &'static str,
    kind: RuleKind,
}

impl Rule {
    const fn new(operation_class: &'static str, kind: RuleKind) -> Self {
        Self {
            operation_class,
            kind,
        }
    }

    fn matches(&self, event: &Event<'_>) -> bool {
        match self.kind {
            RuleKind::Tool(names) => event
                .tool_name
                .is_some_and(|tool_name| contains_ignore_ascii_case(names, tool_name)),
            RuleKind::Git(subcommand) => event
                .command
                .is_some_and(|command| command_matches_git_subcommand(command, subcommand)),
            RuleKind::Command(matcher) => event.command.is_some_and(matcher),
            RuleKind::Output(matcher) => event.output.is_some_and(matcher),
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum RuleKind {
    Tool(&'static [&'static str]),
    Git(&'static str),
    Command(fn(&str) -> bool),
    Output(fn(&str) -> bool),
}

fn default_rules() -> Vec<Rule> {
    vec![
        Rule::new(FILE_READ, RuleKind::Tool(&["Read", "NotebookRead"])),
        Rule::new(FILE_SEARCH, RuleKind::Tool(&["Grep", "Glob"])),
        Rule::new(FILE_LIST, RuleKind::Tool(&["LS"])),
        Rule::new(
            EDIT_ECHO,
            RuleKind::Tool(&["Edit", "MultiEdit", "Write", "NotebookEdit"]),
        ),
        Rule::new(GIT_STATUS, RuleKind::Git("status")),
        Rule::new(GIT_DIFF, RuleKind::Git("diff")),
        Rule::new(GIT_LOG, RuleKind::Git("log")),
        Rule::new(GIT_SHOW, RuleKind::Git("show")),
        Rule::new(GIT_BRANCH, RuleKind::Git("branch")),
        Rule::new(GIT_PUSH, RuleKind::Git("push")),
        Rule::new(GIT_PULL, RuleKind::Git("pull")),
        Rule::new(TEST_OUTPUT, RuleKind::Command(command_is_test)),
        Rule::new(BUILD_OUTPUT, RuleKind::Command(command_is_build)),
        Rule::new(EDIT_ECHO, RuleKind::Command(command_is_edit_echo)),
        Rule::new(FILE_READ, RuleKind::Command(command_is_file_read)),
        Rule::new(FILE_SEARCH, RuleKind::Command(command_is_file_search)),
        Rule::new(FILE_LIST, RuleKind::Command(command_is_file_list)),
        Rule::new(TEST_OUTPUT, RuleKind::Output(output_is_test)),
        Rule::new(BUILD_OUTPUT, RuleKind::Output(output_is_build)),
    ]
}

fn command_matches_git_subcommand(command: &str, expected: &str) -> bool {
    command_segments(command).any(|segment| {
        let tokens = shell_tokens(segment);
        let tokens = command_tokens(&tokens);

        if tokens.first().is_some_and(|token| *token == "git") {
            return git_subcommand(&tokens[1..]).is_some_and(|subcommand| subcommand == expected);
        }

        false
    })
}

fn command_is_test(command: &str) -> bool {
    command_segments(command).any(|segment| {
        let tokens = shell_tokens(segment);
        let tokens = command_tokens(&tokens);

        match tokens.as_slice() {
            ["cargo", "test", ..] | ["cargo", "nextest", "run", ..] => true,
            ["npm", "test", ..] | ["pnpm", "test", ..] | ["yarn", "test", ..] => true,
            ["npm", "run", name, ..] | ["pnpm", "run", name, ..] | ["yarn", "run", name, ..] => {
                name.starts_with("test")
            }
            ["go", "test", ..] => true,
            ["pytest", ..] | ["py.test", ..] => true,
            ["python", "-m", "pytest", ..] | ["python3", "-m", "pytest", ..] => true,
            ["jest", ..] | ["vitest", ..] => true,
            ["mvn", "test", ..] | ["mvn", _, "test", ..] => true,
            ["gradle", "test", ..] | ["./gradlew", "test", ..] | ["gradlew", "test", ..] => true,
            ["make", "test", ..] | ["dotnet", "test", ..] => true,
            _ => false,
        }
    })
}

fn command_is_build(command: &str) -> bool {
    command_segments(command).any(|segment| {
        let tokens = shell_tokens(segment);
        let tokens = command_tokens(&tokens);

        match tokens.as_slice() {
            ["cargo", "build", ..] | ["cargo", "check", ..] | ["cargo", "clippy", ..] => true,
            ["npm", "run", name, ..] | ["pnpm", "run", name, ..] | ["yarn", "run", name, ..] => {
                *name == "build" || name.starts_with("build:")
            }
            ["npm", "build", ..] | ["pnpm", "build", ..] | ["yarn", "build", ..] => true,
            ["go", "build", ..] | ["tsc", ..] | ["rustc", ..] => true,
            ["vite", "build", ..] | ["webpack", ..] | ["rollup", ..] => true,
            ["make", "build", ..] | ["cmake", ..] | ["ninja", ..] => true,
            ["mvn", "package", ..] | ["mvn", "install", ..] => true,
            ["gradle", "build", ..] | ["./gradlew", "build", ..] | ["gradlew", "build", ..] => true,
            ["dotnet", "build", ..] => true,
            _ => false,
        }
    })
}

fn command_is_edit_echo(command: &str) -> bool {
    command_segments(command).any(|segment| {
        let tokens = shell_tokens(segment);
        let tokens = command_tokens(&tokens);

        match tokens.first().copied() {
            Some("apply_patch") => true,
            Some("tee") => tokens.len() > 1,
            Some("echo" | "printf" | "cat") => command_contains_write_redirect(segment),
            _ => false,
        }
    })
}

fn command_is_file_read(command: &str) -> bool {
    command_segments(command).any(|segment| {
        let tokens = shell_tokens(segment);
        let tokens = command_tokens(&tokens);

        matches!(
            tokens.first().copied(),
            Some(
                "cat"
                    | "sed"
                    | "head"
                    | "tail"
                    | "nl"
                    | "less"
                    | "more"
                    | "bat"
                    | "awk"
                    | "wc"
                    | "stat"
                    | "file"
            )
        )
    })
}

fn command_is_file_search(command: &str) -> bool {
    command_segments(command).any(|segment| {
        let tokens = shell_tokens(segment);
        let tokens = command_tokens(&tokens);

        if matches!(
            tokens.first().copied(),
            Some("rg" | "grep" | "ag" | "ack" | "find" | "fd")
        ) {
            return true;
        }

        matches!(tokens.as_slice(), ["git", "grep", ..])
    })
}

fn command_is_file_list(command: &str) -> bool {
    command_segments(command).any(|segment| {
        let tokens = shell_tokens(segment);
        let tokens = command_tokens(&tokens);

        matches!(tokens.first().copied(), Some("ls" | "tree" | "du"))
    })
}

fn output_is_test(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    (lower.contains("running ") && lower.contains(" test"))
        || lower.contains("test result:")
        || lower.contains("tests passed")
        || lower.contains("passed;") && lower.contains("failed;")
        || lower.contains("pytest")
        || lower.contains("jest")
}

fn output_is_build(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("compiling ")
        || lower.contains("finished dev")
        || lower.contains("finished release")
        || lower.contains("build completed")
        || lower.contains("build failed")
        || lower.contains("error[e")
        || lower.contains("webpack compiled")
        || lower.contains("vite v") && lower.contains("built in")
}

fn command_segments(command: &str) -> impl Iterator<Item = &str> {
    command
        .split(['\n', ';', '|'])
        .flat_map(|part| part.split("&&"))
        .flat_map(|part| part.split("||"))
        .map(str::trim)
        .filter(|part| !part.is_empty())
}

fn shell_tokens(segment: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;

    for ch in segment.chars() {
        if escaped {
            current.push(ch.to_ascii_lowercase());
            escaped = false;
            continue;
        }

        match ch {
            '\\' => escaped = true,
            '\'' | '"' if quote == Some(ch) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(ch),
            ch if ch.is_whitespace() && quote.is_none() => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch.to_ascii_lowercase()),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

fn command_tokens(tokens: &[String]) -> Vec<&str> {
    let mut index = 0;

    while let Some(token) = tokens.get(index) {
        if is_env_assignment(token) {
            index += 1;
        } else {
            break;
        }
    }

    while let Some(token) = tokens.get(index) {
        match token.as_str() {
            "sudo" | "command" | "time" => index += 1,
            "env" => {
                index += 1;
                while tokens
                    .get(index)
                    .is_some_and(|token| is_env_assignment(token))
                {
                    index += 1;
                }
            }
            _ => break,
        }
    }

    tokens[index..].iter().map(String::as_str).collect()
}

fn git_subcommand(tokens: &[&str]) -> Option<String> {
    let mut index = 0;

    while let Some(token) = tokens.get(index) {
        match *token {
            "-c" | "-C" | "--git-dir" | "--work-tree" | "--namespace" => index += 2,
            "--no-pager" | "--bare" => index += 1,
            token if token.starts_with('-') => index += 1,
            token => return Some(token.to_string()),
        }
    }

    None
}

fn command_contains_write_redirect(segment: &str) -> bool {
    segment.contains('>') || segment.contains("<<")
}

fn is_env_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };

    !name.is_empty()
        && name
            .chars()
            .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        && name
            .chars()
            .next()
            .is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
}

fn is_generated_path(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();

    if [
        "package-lock.json",
        "npm-shrinkwrap.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "bun.lockb",
        "cargo.lock",
        "gemfile.lock",
        "pipfile.lock",
        "poetry.lock",
        "composer.lock",
        "go.sum",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        return true;
    }

    if lower.contains("/generated/")
        || lower.contains("\\generated\\")
        || lower.contains("/__generated__/")
        || lower.contains("\\__generated__\\")
        || lower.contains(".generated.")
        || lower.contains(".gen.")
        || lower.ends_with(".pb.go")
    {
        return true;
    }

    lower
        .split(['/', '\\', ' ', '\t', '\n', '"', '\'', '`'])
        .any(|part| matches!(part, "generated" | "__generated__" | "codegen"))
}

fn contains_ignore_ascii_case(values: &[&str], needle: &str) -> bool {
    values
        .iter()
        .any(|value| eq_ignore_ascii_case(value, needle))
}

fn eq_ignore_ascii_case(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Fixture<'a> {
        name: &'a str,
        event: Event<'a>,
        expected_class: &'static str,
        expected_generated: bool,
    }

    #[test]
    fn default_rules_classify_fixture_corpus_above_ninety_percent() {
        let classifier = Classifier::default();
        let fixtures = fixture_corpus();
        let mut correct = 0;
        let mut failures = Vec::new();

        for fixture in &fixtures {
            let actual = classifier.classify(&fixture.event);
            let matches = actual.operation_class == fixture.expected_class
                && actual.generated_file == fixture.expected_generated;

            if matches {
                correct += 1;
            } else {
                failures.push(format!(
                    "{}: expected ({}, generated={}), got ({}, generated={})",
                    fixture.name,
                    fixture.expected_class,
                    fixture.expected_generated,
                    actual.operation_class,
                    actual.generated_file
                ));
            }
        }

        let accuracy = correct as f64 / fixtures.len() as f64;
        assert!(
            accuracy >= 0.90,
            "accuracy {accuracy:.2} below 0.90\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn unknown_cases_land_in_other() {
        let classifier = Classifier::default();

        for command in ["date", "curl https://example.test", "whoami"] {
            let actual = classifier.classify_command(command);
            assert_eq!(actual.operation_class, OTHER, "{command}");
            assert!(!actual.generated_file, "{command}");
        }

        let actual = classifier.classify(&Event {
            tool_name: Some("TodoWrite"),
            path: Some("todo list"),
            ..Event::default()
        });
        assert_eq!(actual.operation_class, OTHER);
        assert!(!actual.generated_file);
    }

    fn fixture_corpus() -> Vec<Fixture<'static>> {
        vec![
            bash("git status --short", GIT_STATUS, false),
            bash("git -C repo status --porcelain", GIT_STATUS, false),
            bash("git diff -- src/main.rs", GIT_DIFF, false),
            bash("git --no-pager diff HEAD~1", GIT_DIFF, false),
            bash("git log --oneline -5", GIT_LOG, false),
            bash("git log -- src/main.rs", GIT_LOG, false),
            bash("git show HEAD", GIT_SHOW, false),
            bash("git show HEAD:package-lock.json", GIT_SHOW, true),
            bash("git branch --show-current", GIT_BRANCH, false),
            bash("git push origin HEAD", GIT_PUSH, false),
            bash("git pull --rebase", GIT_PULL, false),
            tool("Read", "src/main.rs", FILE_READ, false),
            tool("NotebookRead", "analysis.ipynb", FILE_READ, false),
            bash("cat Cargo.toml", FILE_READ, false),
            bash("sed -n '1,80p' src/lib.rs", FILE_READ, false),
            bash("head -40 README.md", FILE_READ, false),
            bash("tail -20 server.log", FILE_READ, false),
            bash("nl -ba src/main.rs", FILE_READ, false),
            bash("cat package-lock.json", FILE_READ, true),
            bash("cat src/generated/client.ts", FILE_READ, true),
            tool("Read", "src/__generated__/schema.ts", FILE_READ, true),
            tool("Grep", "pattern=Classifier path=src", FILE_SEARCH, false),
            tool("Glob", "**/*.rs", FILE_SEARCH, false),
            bash("rg 'git status' README.md", FILE_SEARCH, false),
            bash("grep -R \"Classifier\" src", FILE_SEARCH, false),
            bash("find src -name '*.rs'", FILE_SEARCH, false),
            bash("git grep classifier", FILE_SEARCH, false),
            tool("LS", "src", FILE_LIST, false),
            bash("ls -la src", FILE_LIST, false),
            bash("tree src", FILE_LIST, false),
            bash("cargo test", TEST_OUTPUT, false),
            bash("npm test -- --watch=false", TEST_OUTPUT, false),
            bash("npm run test:unit", TEST_OUTPUT, false),
            bash("python -m pytest tests", TEST_OUTPUT, false),
            bash("go test ./...", TEST_OUTPUT, false),
            output(
                "running 12 tests\ntest result: ok. 12 passed",
                TEST_OUTPUT,
                false,
            ),
            bash("cargo build --release", BUILD_OUTPUT, false),
            bash("cargo check", BUILD_OUTPUT, false),
            bash("npm run build", BUILD_OUTPUT, false),
            bash("pnpm run build:web", BUILD_OUTPUT, false),
            bash("go build ./cmd/app", BUILD_OUTPUT, false),
            bash("tsc --noEmit", BUILD_OUTPUT, false),
            output(
                "Compiling vc-tokmeter v0.1.0\nFinished dev profile",
                BUILD_OUTPUT,
                false,
            ),
            tool("Edit", "src/main.rs", EDIT_ECHO, false),
            tool("MultiEdit", "src/lib.rs", EDIT_ECHO, false),
            tool("Write", "src/generated/types.ts", EDIT_ECHO, true),
            bash("echo 'pub mod classifier;' > src/lib.rs", EDIT_ECHO, false),
            bash("printf '%s\n' hello >> notes.txt", EDIT_ECHO, false),
            bash(
                "cat <<'EOF' > src/main.rs\nfn main() {}\nEOF",
                EDIT_ECHO,
                false,
            ),
            bash("tee src/main.rs", EDIT_ECHO, false),
        ]
    }

    fn bash(
        command: &'static str,
        expected_class: &'static str,
        generated: bool,
    ) -> Fixture<'static> {
        Fixture {
            name: command,
            event: Event {
                tool_name: Some("Bash"),
                command: Some(command),
                ..Event::default()
            },
            expected_class,
            expected_generated: generated,
        }
    }

    fn tool(
        tool_name: &'static str,
        path: &'static str,
        expected_class: &'static str,
        generated: bool,
    ) -> Fixture<'static> {
        Fixture {
            name: tool_name,
            event: Event {
                tool_name: Some(tool_name),
                path: Some(path),
                ..Event::default()
            },
            expected_class,
            expected_generated: generated,
        }
    }

    fn output(
        output: &'static str,
        expected_class: &'static str,
        generated: bool,
    ) -> Fixture<'static> {
        Fixture {
            name: output,
            event: Event {
                output: Some(output),
                ..Event::default()
            },
            expected_class,
            expected_generated: generated,
        }
    }
}
