#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedTranscript {
    pub counts: TranscriptCounts,
    pub diagnostics: Vec<TranscriptDiagnostic>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TranscriptCounts {
    pub total_lines: usize,
    pub tool_calls: usize,
    pub tool_results: usize,
    pub usage: usize,
    pub unknown: usize,
    pub malformed: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptDiagnostic {
    pub line: usize,
    pub kind: TranscriptDiagnosticKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptDiagnosticKind {
    MalformedJsonlRecord,
    UnknownRecord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecordKind {
    ToolCall,
    ToolResult,
    Usage,
    Unknown,
}

pub fn parse_transcript(input: &str) -> ParsedTranscript {
    let mut parsed = ParsedTranscript::default();

    for (index, line) in input.lines().enumerate() {
        let line_number = index + 1;
        let trimmed = line.trim();

        if trimmed.is_empty() {
            continue;
        }

        parsed.counts.total_lines += 1;

        if !looks_like_json_object(trimmed) {
            parsed.counts.malformed += 1;
            parsed.diagnostics.push(TranscriptDiagnostic {
                line: line_number,
                kind: TranscriptDiagnosticKind::MalformedJsonlRecord,
            });
            continue;
        }

        match classify_record(trimmed) {
            RecordKind::ToolCall => parsed.counts.tool_calls += 1,
            RecordKind::ToolResult => parsed.counts.tool_results += 1,
            RecordKind::Usage => parsed.counts.usage += 1,
            RecordKind::Unknown => {
                parsed.counts.unknown += 1;
                parsed.diagnostics.push(TranscriptDiagnostic {
                    line: line_number,
                    kind: TranscriptDiagnosticKind::UnknownRecord,
                });
            }
        }
    }

    parsed
}

fn looks_like_json_object(line: &str) -> bool {
    line.starts_with('{') && line.ends_with('}') && braces_are_balanced(line)
}

fn braces_are_balanced(line: &str) -> bool {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for byte in line.bytes() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }

        match byte {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
            }
            _ => {}
        }
    }

    depth == 0 && !in_string
}

fn classify_record(line: &str) -> RecordKind {
    if has_any_marker(
        line,
        &[
            "\"tool_call\"",
            "\"tool_calls\"",
            "\"toolUse\"",
            "\"tool_use\"",
            "\"function_call\"",
            "\"recipient_name\"",
        ],
    ) {
        return RecordKind::ToolCall;
    }

    if has_any_marker(
        line,
        &[
            "\"tool_result\"",
            "\"tool_results\"",
            "\"toolResult\"",
            "\"tool_output\"",
            "\"function_call_output\"",
        ],
    ) {
        return RecordKind::ToolResult;
    }

    if has_any_marker(
        line,
        &[
            "\"usage\"",
            "\"token_usage\"",
            "\"input_tokens\"",
            "\"output_tokens\"",
            "\"prompt_tokens\"",
            "\"completion_tokens\"",
        ],
    ) {
        return RecordKind::Usage;
    }

    RecordKind::Unknown
}

fn has_any_marker(line: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| line.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_known_record_types() {
        let input = concat!(
            "{\"type\":\"tool_call\",\"name\":\"shell\"}\n",
            "{\"type\":\"tool_result\",\"status\":\"ok\"}\n",
            "{\"usage\":{\"input_tokens\":12,\"output_tokens\":4}}\n",
        );

        let parsed = parse_transcript(input);

        assert_eq!(parsed.counts.total_lines, 3);
        assert_eq!(parsed.counts.tool_calls, 1);
        assert_eq!(parsed.counts.tool_results, 1);
        assert_eq!(parsed.counts.usage, 1);
        assert_eq!(parsed.counts.unknown, 0);
        assert_eq!(parsed.counts.malformed, 0);
        assert!(parsed.diagnostics.is_empty());
    }

    #[test]
    fn reports_unknown_records_without_storing_content() {
        let input = "{\"type\":\"message\",\"content\":\"do not retain me\"}\n";

        let parsed = parse_transcript(input);

        assert_eq!(parsed.counts.total_lines, 1);
        assert_eq!(parsed.counts.unknown, 1);
        assert_eq!(
            parsed.diagnostics,
            vec![TranscriptDiagnostic {
                line: 1,
                kind: TranscriptDiagnosticKind::UnknownRecord,
            }]
        );
    }

    #[test]
    fn malformed_lines_are_recoverable() {
        let input = concat!(
            "{\"type\":\"tool_call\",\"name\":\"shell\"}\n",
            "this is not json\n",
            "{\"type\":\"tool_result\",\"status\":\"ok\"}\n",
        );

        let parsed = parse_transcript(input);

        assert_eq!(parsed.counts.total_lines, 3);
        assert_eq!(parsed.counts.tool_calls, 1);
        assert_eq!(parsed.counts.tool_results, 1);
        assert_eq!(parsed.counts.malformed, 1);
        assert_eq!(
            parsed.diagnostics,
            vec![TranscriptDiagnostic {
                line: 2,
                kind: TranscriptDiagnosticKind::MalformedJsonlRecord,
            }]
        );
    }

    #[test]
    fn malformed_objects_with_unclosed_strings_are_diagnostics() {
        let input = concat!(
            "{\"type\":\"tool_call\",\"arguments\":\"unterminated}\n",
            "{\"usage\":{\"input_tokens\":1}}\n",
        );

        let parsed = parse_transcript(input);

        assert_eq!(parsed.counts.total_lines, 2);
        assert_eq!(parsed.counts.malformed, 1);
        assert_eq!(parsed.counts.usage, 1);
        assert_eq!(parsed.diagnostics[0].line, 1);
        assert_eq!(
            parsed.diagnostics[0].kind,
            TranscriptDiagnosticKind::MalformedJsonlRecord
        );
    }

    #[test]
    fn ignores_blank_lines() {
        let parsed = parse_transcript("\n  \n{\"type\":\"function_call\"}\n");

        assert_eq!(parsed.counts.total_lines, 1);
        assert_eq!(parsed.counts.tool_calls, 1);
        assert!(parsed.diagnostics.is_empty());
    }
}
