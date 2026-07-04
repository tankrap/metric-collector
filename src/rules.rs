use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;

const VALID_OP_CLASSES: &[&str] = &[
    "vc.status",
    "vc.diff",
    "vc.log",
    "vc.show",
    "vc.branch_ops",
    "vc.push_pull",
    "file.read",
    "file.search",
    "file.list",
    "edit.echo",
    "test.output",
    "build.output",
    "session.meta",
    "other",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleMode {
    Contains,
    Prefix,
    Suffix,
}

impl RuleMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            RuleMode::Contains => "contains",
            RuleMode::Prefix => "prefix",
            RuleMode::Suffix => "suffix",
        }
    }
}

impl Default for RuleMode {
    fn default() -> Self {
        Self::Contains
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub name: String,
    pub pattern: String,
    pub op_class: String,
    pub generated: Option<bool>,
    pub mode: RuleMode,
}

impl Rule {
    pub fn new(
        name: impl Into<String>,
        pattern: impl Into<String>,
        op_class: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            pattern: pattern.into(),
            op_class: op_class.into(),
            generated: None,
            mode: RuleMode::default(),
        }
    }

    pub fn matches(&self, value: &str) -> bool {
        match self.mode {
            RuleMode::Contains => value.contains(&self.pattern),
            RuleMode::Prefix => value.starts_with(&self.pattern),
            RuleMode::Suffix => value.ends_with(&self.pattern),
        }
    }
}

pub fn parse_user_rules(input: &str) -> Result<Vec<Rule>, RuleParseError> {
    let mut parser = Parser::new(input);
    let mut rules = parser.parse()?;

    for rule in &mut rules {
        rule.name = rule.name.trim().to_owned();
        rule.pattern = rule.pattern.trim().to_owned();
        rule.op_class = rule.op_class.trim().to_owned();
    }

    validate_rules(&rules).map_err(RuleParseError::Validation)?;

    Ok(rules)
}

pub fn validate_rules(rules: &[Rule]) -> Result<(), RuleValidationError> {
    let mut names = HashSet::new();

    for rule in rules {
        validate_non_empty("name", &rule.name)?;

        let pattern = rule.pattern.trim();
        if pattern.is_empty() {
            return Err(RuleValidationError::InvalidPattern {
                name: rule.name.clone(),
            });
        }

        if pattern.contains('\0') || pattern.chars().any(char::is_control) {
            return Err(RuleValidationError::InvalidPattern {
                name: rule.name.clone(),
            });
        }

        validate_non_empty("op_class", &rule.op_class)?;
        if !VALID_OP_CLASSES.contains(&rule.op_class.as_str()) {
            return Err(RuleValidationError::InvalidOpClass {
                name: rule.name.clone(),
                op_class: rule.op_class.clone(),
            });
        }

        if !names.insert(rule.name.as_str()) {
            return Err(RuleValidationError::DuplicateName {
                name: rule.name.clone(),
            });
        }
    }

    Ok(())
}

pub fn merge_rules(
    default_rules: &[Rule],
    user_rules: &[Rule],
) -> Result<Vec<Rule>, RuleMergeError> {
    validate_rules(default_rules).map_err(RuleMergeError::InvalidDefaultRules)?;
    validate_rules(user_rules).map_err(RuleMergeError::InvalidUserRules)?;

    let mut merged = default_rules.to_vec();
    let mut indexes = HashMap::new();

    for (index, rule) in merged.iter().enumerate() {
        indexes.insert(rule.name.clone(), index);
    }

    for rule in user_rules {
        if let Some(index) = indexes.get(&rule.name).copied() {
            merged[index] = rule.clone();
        } else {
            indexes.insert(rule.name.clone(), merged.len());
            merged.push(rule.clone());
        }
    }

    Ok(merged)
}

pub fn is_valid_op_class(value: &str) -> bool {
    VALID_OP_CLASSES.contains(&value)
}

fn validate_non_empty(field: &'static str, value: &str) -> Result<(), RuleValidationError> {
    if value.trim().is_empty() {
        return Err(RuleValidationError::MissingField { rule: None, field });
    }

    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleParseError {
    ExpectedRulesHeader { line: usize },
    ExpectedRuleEntry { line: usize },
    ExpectedKeyValue { line: usize },
    UnknownField { line: usize, field: String },
    DuplicateField { line: usize, field: String },
    InvalidBoolean { line: usize, value: String },
    InvalidMode { line: usize, value: String },
    MissingField { line: usize, field: &'static str },
    Validation(RuleValidationError),
}

impl fmt::Display for RuleParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleParseError::ExpectedRulesHeader { line } => {
                write!(formatter, "line {line}: expected 'rules:'")
            }
            RuleParseError::ExpectedRuleEntry { line } => {
                write!(
                    formatter,
                    "line {line}: expected a rule entry starting with '-'"
                )
            }
            RuleParseError::ExpectedKeyValue { line } => {
                write!(formatter, "line {line}: expected 'key: value'")
            }
            RuleParseError::UnknownField { line, field } => {
                write!(formatter, "line {line}: unknown field '{field}'")
            }
            RuleParseError::DuplicateField { line, field } => {
                write!(formatter, "line {line}: duplicate field '{field}'")
            }
            RuleParseError::InvalidBoolean { line, value } => {
                write!(formatter, "line {line}: invalid boolean '{value}'")
            }
            RuleParseError::InvalidMode { line, value } => {
                write!(formatter, "line {line}: invalid mode '{value}'")
            }
            RuleParseError::MissingField { line, field } => {
                write!(formatter, "line {line}: missing field '{field}'")
            }
            RuleParseError::Validation(error) => error.fmt(formatter),
        }
    }
}

impl Error for RuleParseError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleValidationError {
    MissingField {
        rule: Option<String>,
        field: &'static str,
    },
    DuplicateName {
        name: String,
    },
    InvalidOpClass {
        name: String,
        op_class: String,
    },
    InvalidPattern {
        name: String,
    },
}

impl fmt::Display for RuleValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleValidationError::MissingField { rule, field } => {
                if let Some(rule) = rule {
                    write!(formatter, "rule '{rule}' is missing field '{field}'")
                } else {
                    write!(formatter, "rule is missing field '{field}'")
                }
            }
            RuleValidationError::DuplicateName { name } => {
                write!(formatter, "duplicate rule name '{name}'")
            }
            RuleValidationError::InvalidOpClass { name, op_class } => {
                write!(formatter, "rule '{name}' has invalid op_class '{op_class}'")
            }
            RuleValidationError::InvalidPattern { name } => {
                write!(formatter, "rule '{name}' has an invalid pattern")
            }
        }
    }
}

impl Error for RuleValidationError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleMergeError {
    InvalidDefaultRules(RuleValidationError),
    InvalidUserRules(RuleValidationError),
}

impl fmt::Display for RuleMergeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RuleMergeError::InvalidDefaultRules(error) => {
                write!(formatter, "invalid default rules: {error}")
            }
            RuleMergeError::InvalidUserRules(error) => {
                write!(formatter, "invalid user rules: {error}")
            }
        }
    }
}

impl Error for RuleMergeError {}

struct Parser<'a> {
    input: &'a str,
}

impl<'a> Parser<'a> {
    const fn new(input: &'a str) -> Self {
        Self { input }
    }

    fn parse(&mut self) -> Result<Vec<Rule>, RuleParseError> {
        let mut rules = Vec::new();
        let mut current: Option<PartialRule> = None;
        let mut in_rules = false;

        for (index, raw_line) in self.input.lines().enumerate() {
            let line_number = index + 1;
            let line = strip_comment(raw_line).trim();

            if line.is_empty() {
                continue;
            }

            if line == "rules:" {
                if let Some(partial) = current.take() {
                    rules.push(partial.finish()?);
                }
                in_rules = true;
                continue;
            }

            if !in_rules && !line.starts_with("- ") && line != "-" {
                return Err(RuleParseError::ExpectedRulesHeader { line: line_number });
            }

            if line == "-" || line.starts_with("- ") {
                if let Some(partial) = current.take() {
                    rules.push(partial.finish()?);
                }

                let mut partial = PartialRule::new(line_number);
                let rest = line.trim_start_matches('-').trim();
                if !rest.is_empty() {
                    partial.set(line_number, rest)?;
                }
                current = Some(partial);
                in_rules = true;
                continue;
            }

            let partial = current
                .as_mut()
                .ok_or(RuleParseError::ExpectedRuleEntry { line: line_number })?;
            partial.set(line_number, line)?;
        }

        if let Some(partial) = current {
            rules.push(partial.finish()?);
        }

        Ok(rules)
    }
}

#[derive(Debug, Clone)]
struct PartialRule {
    line: usize,
    name: Option<String>,
    pattern: Option<String>,
    op_class: Option<String>,
    generated: Option<bool>,
    mode: Option<RuleMode>,
    seen_fields: HashSet<&'static str>,
}

impl PartialRule {
    fn new(line: usize) -> Self {
        Self {
            line,
            name: None,
            pattern: None,
            op_class: None,
            generated: None,
            mode: None,
            seen_fields: HashSet::new(),
        }
    }

    fn set(&mut self, line: usize, text: &str) -> Result<(), RuleParseError> {
        let (field, value) = text
            .split_once(':')
            .ok_or(RuleParseError::ExpectedKeyValue { line })?;
        let field = field.trim();
        let value = unquote(value.trim());

        match field {
            "name" => self.set_string(line, "name", value, |rule, value| rule.name = Some(value)),
            "pattern" => self.set_string(line, "pattern", value, |rule, value| {
                rule.pattern = Some(value);
            }),
            "op_class" => self.set_string(line, "op_class", value, |rule, value| {
                rule.op_class = Some(value);
            }),
            "generated" => {
                self.mark_seen(line, "generated")?;
                self.generated = Some(parse_bool(line, &value)?);
                Ok(())
            }
            "mode" => {
                self.mark_seen(line, "mode")?;
                self.mode = Some(parse_mode(line, &value)?);
                Ok(())
            }
            _ => Err(RuleParseError::UnknownField {
                line,
                field: field.to_owned(),
            }),
        }
    }

    fn set_string(
        &mut self,
        line: usize,
        field: &'static str,
        value: String,
        assign: impl FnOnce(&mut Self, String),
    ) -> Result<(), RuleParseError> {
        self.mark_seen(line, field)?;
        assign(self, value);
        Ok(())
    }

    fn mark_seen(&mut self, line: usize, field: &'static str) -> Result<(), RuleParseError> {
        if !self.seen_fields.insert(field) {
            return Err(RuleParseError::DuplicateField {
                line,
                field: field.to_owned(),
            });
        }

        Ok(())
    }

    fn finish(self) -> Result<Rule, RuleParseError> {
        let line = self.line;
        let name = required(line, "name", self.name)?;
        let pattern = required(line, "pattern", self.pattern)?;
        let op_class = required(line, "op_class", self.op_class)?;

        Ok(Rule {
            name,
            pattern,
            op_class,
            generated: self.generated,
            mode: self.mode.unwrap_or_default(),
        })
    }
}

fn required(
    line: usize,
    field: &'static str,
    value: Option<String>,
) -> Result<String, RuleParseError> {
    value.ok_or(RuleParseError::MissingField { line, field })
}

fn parse_bool(line: usize, value: &str) -> Result<bool, RuleParseError> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(RuleParseError::InvalidBoolean {
            line,
            value: value.to_owned(),
        }),
    }
}

fn parse_mode(line: usize, value: &str) -> Result<RuleMode, RuleParseError> {
    match value {
        "contains" => Ok(RuleMode::Contains),
        "prefix" => Ok(RuleMode::Prefix),
        "suffix" => Ok(RuleMode::Suffix),
        _ => Err(RuleParseError::InvalidMode {
            line,
            value: value.to_owned(),
        }),
    }
}

fn strip_comment(line: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;

    for (index, ch) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }

        match ch {
            '\\' if quote == Some('"') => escaped = true,
            '\'' | '"' if quote == Some(ch) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(ch),
            '#' if quote.is_none() => return &line[..index],
            _ => {}
        }
    }

    line
}

fn unquote(value: &str) -> String {
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        let first = bytes[0];
        let last = bytes[value.len() - 1];

        if (first == b'\'' && last == b'\'') || (first == b'"' && last == b'"') {
            return value[1..value.len() - 1].to_owned();
        }
    }

    value.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rules_with_optional_fields_and_comments() {
        let rules = parse_user_rules(
            r#"
            # user rules
            rules:
              - name: generated-client
                pattern: "src/generated/"
                op_class: file.read
                generated: true
                mode: prefix
              - name: rust-tests
                pattern: cargo test # trailing comment
                op_class: test.output
            "#,
        )
        .unwrap();

        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].name, "generated-client");
        assert_eq!(rules[0].pattern, "src/generated/");
        assert_eq!(rules[0].op_class, "file.read");
        assert_eq!(rules[0].generated, Some(true));
        assert_eq!(rules[0].mode, RuleMode::Prefix);
        assert_eq!(rules[1].name, "rust-tests");
        assert_eq!(rules[1].generated, None);
        assert_eq!(rules[1].mode, RuleMode::Contains);
    }

    #[test]
    fn parses_list_without_rules_header() {
        let rules = parse_user_rules(
            r#"
            - name: shell-list
              pattern: ls
              op_class: file.list
            "#,
        )
        .unwrap();

        assert_eq!(rules, vec![Rule::new("shell-list", "ls", "file.list")]);
    }

    #[test]
    fn supports_inline_first_field() {
        let rules = parse_user_rules(
            r#"
            rules:
              - name: cargo-build
                pattern: cargo build
                op_class: build.output
                mode: contains
            "#,
        )
        .unwrap();

        assert_eq!(rules[0].name, "cargo-build");
        assert_eq!(rules[0].mode, RuleMode::Contains);
    }

    #[test]
    fn reports_missing_required_fields() {
        let error = parse_user_rules(
            r#"
            rules:
              - name: incomplete
                pattern: rg
            "#,
        )
        .unwrap_err();

        assert_eq!(
            error,
            RuleParseError::MissingField {
                line: 3,
                field: "op_class"
            }
        );
    }

    #[test]
    fn rejects_duplicate_names() {
        let error = parse_user_rules(
            r#"
            rules:
              - name: duplicate
                pattern: rg
                op_class: file.search
              - name: duplicate
                pattern: grep
                op_class: file.search
            "#,
        )
        .unwrap_err();

        assert_eq!(
            error,
            RuleParseError::Validation(RuleValidationError::DuplicateName {
                name: "duplicate".to_owned()
            })
        );
    }

    #[test]
    fn rejects_duplicate_names_after_trimming() {
        let error = parse_user_rules(
            r#"
            rules:
              - name: "duplicate "
                pattern: rg
                op_class: file.search
              - name: duplicate
                pattern: grep
                op_class: file.search
            "#,
        )
        .unwrap_err();

        assert_eq!(
            error,
            RuleParseError::Validation(RuleValidationError::DuplicateName {
                name: "duplicate".to_owned()
            })
        );
    }

    #[test]
    fn rejects_invalid_op_class() {
        let error = parse_user_rules(
            r#"
            rules:
              - name: bad-class
                pattern: cargo doc
                op_class: docs.output
            "#,
        )
        .unwrap_err();

        assert_eq!(
            error,
            RuleParseError::Validation(RuleValidationError::InvalidOpClass {
                name: "bad-class".to_owned(),
                op_class: "docs.output".to_owned()
            })
        );
    }

    #[test]
    fn rejects_empty_patterns() {
        let error = parse_user_rules(
            r#"
            rules:
              - name: empty-pattern
                pattern: "   "
                op_class: file.read
            "#,
        )
        .unwrap_err();

        assert_eq!(
            error,
            RuleParseError::Validation(RuleValidationError::InvalidPattern {
                name: "empty-pattern".to_owned()
            })
        );
    }

    #[test]
    fn rejects_control_character_patterns() {
        let error =
            validate_rules(&[Rule::new("bad-pattern", "abc\nxyz", "file.read")]).unwrap_err();

        assert_eq!(
            error,
            RuleValidationError::InvalidPattern {
                name: "bad-pattern".to_owned()
            }
        );
    }

    #[test]
    fn rejects_invalid_modes() {
        let error = parse_user_rules(
            r#"
            rules:
              - name: bad-mode
                pattern: cargo
                op_class: build.output
                mode: regex
            "#,
        )
        .unwrap_err();

        assert_eq!(
            error,
            RuleParseError::InvalidMode {
                line: 6,
                value: "regex".to_owned()
            }
        );
    }

    #[test]
    fn rejects_invalid_booleans() {
        let error = parse_user_rules(
            r#"
            rules:
              - name: bad-generated
                pattern: Cargo.lock
                op_class: file.read
                generated: yes
            "#,
        )
        .unwrap_err();

        assert_eq!(
            error,
            RuleParseError::InvalidBoolean {
                line: 6,
                value: "yes".to_owned()
            }
        );
    }

    #[test]
    fn rejects_duplicate_fields() {
        let error = parse_user_rules(
            r#"
            rules:
              - name: duplicate-field
                name: duplicate-field-again
                pattern: rg
                op_class: file.search
            "#,
        )
        .unwrap_err();

        assert_eq!(
            error,
            RuleParseError::DuplicateField {
                line: 4,
                field: "name".to_owned()
            }
        );
    }

    #[test]
    fn rejects_unknown_fields() {
        let error = parse_user_rules(
            r#"
            rules:
              - name: unknown-field
                pattern: rg
                op_class: file.search
                weight: 2
            "#,
        )
        .unwrap_err();

        assert_eq!(
            error,
            RuleParseError::UnknownField {
                line: 6,
                field: "weight".to_owned()
            }
        );
    }

    #[test]
    fn matches_using_contains_prefix_and_suffix_modes() {
        let contains = Rule::new("contains", "generated", "file.read");
        let prefix = Rule {
            mode: RuleMode::Prefix,
            ..Rule::new("prefix", "src/", "file.read")
        };
        let suffix = Rule {
            mode: RuleMode::Suffix,
            ..Rule::new("suffix", ".lock", "file.read")
        };

        assert!(contains.matches("src/generated/client.rs"));
        assert!(prefix.matches("src/lib.rs"));
        assert!(!prefix.matches("tests/src/lib.rs"));
        assert!(suffix.matches("Cargo.lock"));
        assert!(!suffix.matches("Cargo.toml"));
    }

    #[test]
    fn merge_overrides_defaults_in_place_and_appends_new_rules() {
        let defaults = vec![
            Rule::new("git-status", "git status", "vc.status"),
            Rule::new("read", "cat ", "file.read"),
            Rule::new("search", "rg ", "file.search"),
        ];
        let users = vec![
            Rule {
                mode: RuleMode::Prefix,
                ..Rule::new("read", "sed ", "file.read")
            },
            Rule::new("test", "cargo test", "test.output"),
        ];

        let merged = merge_rules(&defaults, &users).unwrap();

        assert_eq!(
            merged
                .iter()
                .map(|rule| rule.name.as_str())
                .collect::<Vec<_>>(),
            vec!["git-status", "read", "search", "test"]
        );
        assert_eq!(merged[1].pattern, "sed ");
        assert_eq!(merged[1].mode, RuleMode::Prefix);
        assert_eq!(merged[3].op_class, "test.output");
    }

    #[test]
    fn merge_rejects_invalid_user_rules() {
        let defaults = vec![Rule::new("read", "cat ", "file.read")];
        let users = vec![Rule::new("bad", "cargo doc", "docs.output")];

        let error = merge_rules(&defaults, &users).unwrap_err();

        assert_eq!(
            error,
            RuleMergeError::InvalidUserRules(RuleValidationError::InvalidOpClass {
                name: "bad".to_owned(),
                op_class: "docs.output".to_owned()
            })
        );
    }

    #[test]
    fn frozen_taxonomy_contains_expected_op_classes() {
        assert_eq!(
            VALID_OP_CLASSES,
            &[
                "vc.status",
                "vc.diff",
                "vc.log",
                "vc.show",
                "vc.branch_ops",
                "vc.push_pull",
                "file.read",
                "file.search",
                "file.list",
                "edit.echo",
                "test.output",
                "build.output",
                "session.meta",
                "other",
            ]
        );
        assert!(is_valid_op_class("file.read"));
        assert!(!is_valid_op_class("file.write"));
    }
}
