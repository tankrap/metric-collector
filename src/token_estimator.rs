use std::fmt;

pub const TOKENIZER_STRATEGY: &str = "approx-universal-v1";
pub const TOKENIZER_LABEL: &str = "deterministic approximate universal tokenizer";
pub const CALIBRATION_LABEL: &str = "aggregate reported / estimated calibration";
pub const DIGEST_ALGORITHM: &str = "fnv1a64";

const FNV1A64_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV1A64_PRIME: u64 = 0x0000_0100_0000_01b3;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PayloadKind {
    Empty,
    Text,
    Code,
    Structured,
}

impl PayloadKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            PayloadKind::Empty => "empty",
            PayloadKind::Text => "text",
            PayloadKind::Code => "code",
            PayloadKind::Structured => "structured",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct EstimatedTokenCounts {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
}

impl EstimatedTokenCounts {
    pub const fn output(output_tokens: u64) -> Self {
        Self {
            input_tokens: 0,
            output_tokens,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
        }
    }

    pub fn total(self) -> u64 {
        self.input_tokens
            .saturating_add(self.output_tokens)
            .saturating_add(self.cache_read_tokens)
            .saturating_add(self.cache_write_tokens)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalibrationFactor {
    pub numerator: u64,
    pub denominator: u64,
    pub sample_count: u64,
}

impl CalibrationFactor {
    pub fn new(numerator: u64, denominator: u64, sample_count: u64) -> Option<Self> {
        (numerator > 0 && denominator > 0).then_some(Self {
            numerator,
            denominator,
            sample_count,
        })
    }

    pub fn apply(self, estimated_tokens: u64) -> u64 {
        if estimated_tokens == 0 {
            return 0;
        }

        let numerator = u128::from(estimated_tokens).saturating_mul(u128::from(self.numerator));
        let rounded = numerator.saturating_add(u128::from(self.denominator / 2))
            / u128::from(self.denominator);
        rounded.min(u128::from(u64::MAX)) as u64
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalibrationSample {
    pub estimated_tokens: u64,
    pub reported_tokens: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TokenEstimatorConfig {
    pub calibration: Option<CalibrationFactor>,
}

impl TokenEstimatorConfig {
    pub const fn uncalibrated() -> Self {
        Self { calibration: None }
    }

    pub const fn calibrated(calibration: CalibrationFactor) -> Self {
        Self {
            calibration: Some(calibration),
        }
    }
}

impl Default for TokenEstimatorConfig {
    fn default() -> Self {
        Self::uncalibrated()
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct EstimateMetadata {
    pub estimated: bool,
    pub strategy: &'static str,
    pub tokenizer: &'static str,
    pub provider_specific: bool,
    pub payload_kind: PayloadKind,
    pub byte_count: u64,
    pub char_count: u64,
    pub line_count: u64,
    pub calibration: Option<CalibrationMetadata>,
}

impl EstimateMetadata {
    pub fn pairs(&self) -> Vec<(&'static str, String)> {
        let mut pairs = vec![
            ("estimated", self.estimated.to_string()),
            ("strategy", self.strategy.to_owned()),
            ("tokenizer", self.tokenizer.to_owned()),
            ("provider_specific", self.provider_specific.to_string()),
            ("payload_kind", self.payload_kind.as_str().to_owned()),
            ("byte_count", self.byte_count.to_string()),
            ("char_count", self.char_count.to_string()),
            ("line_count", self.line_count.to_string()),
        ];

        if let Some(calibration) = self.calibration {
            pairs.extend([
                ("calibration", calibration.method.to_owned()),
                (
                    "calibration_numerator",
                    calibration.factor.numerator.to_string(),
                ),
                (
                    "calibration_denominator",
                    calibration.factor.denominator.to_string(),
                ),
                (
                    "calibration_sample_count",
                    calibration.factor.sample_count.to_string(),
                ),
            ]);
        }

        pairs
    }
}

impl fmt::Debug for EstimateMetadata {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("EstimateMetadata")
            .field("estimated", &self.estimated)
            .field("strategy", &self.strategy)
            .field("tokenizer", &self.tokenizer)
            .field("provider_specific", &self.provider_specific)
            .field("payload_kind", &self.payload_kind)
            .field("byte_count", &self.byte_count)
            .field("char_count", &self.char_count)
            .field("line_count", &self.line_count)
            .field("calibration", &self.calibration)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CalibrationMetadata {
    pub method: &'static str,
    pub factor: CalibrationFactor,
}

#[derive(Clone, Eq, PartialEq)]
pub struct ToolResultTokenEstimate {
    pub tokens: EstimatedTokenCounts,
    pub raw_estimated_tokens: u64,
    pub content_digest: String,
    pub metadata: EstimateMetadata,
}

impl ToolResultTokenEstimate {
    pub fn metadata_pairs(&self) -> Vec<(&'static str, String)> {
        let mut pairs = self.metadata.pairs();
        pairs.extend([
            ("content_digest", self.content_digest.clone()),
            (
                "raw_estimated_tokens",
                self.raw_estimated_tokens.to_string(),
            ),
            ("output_tokens", self.tokens.output_tokens.to_string()),
            ("total_tokens", self.tokens.total().to_string()),
        ]);
        pairs
    }
}

impl fmt::Debug for ToolResultTokenEstimate {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ToolResultTokenEstimate")
            .field("tokens", &self.tokens)
            .field("raw_estimated_tokens", &self.raw_estimated_tokens)
            .field("content_digest", &self.content_digest)
            .field("metadata", &self.metadata)
            .finish()
    }
}

pub fn estimate_tool_result_tokens(payload: &str) -> ToolResultTokenEstimate {
    estimate_tool_result_tokens_with_config(payload, TokenEstimatorConfig::default())
}

pub fn estimate_tool_result_tokens_with_config(
    payload: &str,
    config: TokenEstimatorConfig,
) -> ToolResultTokenEstimate {
    let features = PayloadFeatures::analyze(payload);
    let raw_estimated_tokens = estimate_universal_tokens(&features);
    let output_tokens = config
        .calibration
        .map(|factor| factor.apply(raw_estimated_tokens))
        .unwrap_or(raw_estimated_tokens);

    ToolResultTokenEstimate {
        tokens: EstimatedTokenCounts::output(output_tokens),
        raw_estimated_tokens,
        content_digest: digest_with_domain("tool-result", payload),
        metadata: EstimateMetadata {
            estimated: true,
            strategy: TOKENIZER_STRATEGY,
            tokenizer: TOKENIZER_LABEL,
            provider_specific: false,
            payload_kind: features.kind,
            byte_count: to_u64(payload.len()),
            char_count: features.char_count,
            line_count: features.line_count,
            calibration: config.calibration.map(|factor| CalibrationMetadata {
                method: CALIBRATION_LABEL,
                factor,
            }),
        },
    }
}

pub fn calibration_from_samples(samples: &[CalibrationSample]) -> Option<CalibrationFactor> {
    let mut estimated_total = 0u64;
    let mut reported_total = 0u64;
    let mut sample_count = 0u64;

    for sample in samples {
        if sample.estimated_tokens == 0 || sample.reported_tokens == 0 {
            continue;
        }

        estimated_total = estimated_total.saturating_add(sample.estimated_tokens);
        reported_total = reported_total.saturating_add(sample.reported_tokens);
        sample_count = sample_count.saturating_add(1);
    }

    CalibrationFactor::new(reported_total, estimated_total, sample_count)
}

pub fn estimate_universal_payload_tokens(payload: &str) -> u64 {
    estimate_universal_tokens(&PayloadFeatures::analyze(payload))
}

fn estimate_universal_tokens(features: &PayloadFeatures) -> u64 {
    if features.kind == PayloadKind::Empty {
        return 0;
    }

    let base = div_ceil(features.char_count, 4);
    let lexical = features
        .word_runs
        .saturating_add(div_ceil(features.number_runs, 2));
    let structure =
        div_ceil(features.symbol_count, 3).saturating_add(features.line_count.saturating_sub(1));
    let non_ascii = div_ceil(features.non_ascii_bytes, 2);
    let whitespace_discount = div_ceil(features.whitespace_count, 24);

    let tokens = match features.kind {
        PayloadKind::Empty => 0,
        PayloadKind::Text => base
            .max(lexical)
            .saturating_add(div_ceil(features.punctuation_count, 5))
            .saturating_add(non_ascii)
            .saturating_sub(whitespace_discount),
        PayloadKind::Structured => base
            .max(lexical.saturating_add(structure))
            .saturating_add(div_ceil(features.punctuation_count, 4))
            .saturating_add(non_ascii),
        PayloadKind::Code => base
            .max(lexical.saturating_add(structure))
            .saturating_add(div_ceil(features.operator_count, 2))
            .saturating_add(non_ascii),
    };

    tokens.max(1)
}

fn digest_with_domain(domain: &str, payload: &str) -> String {
    let mut hash = FNV1A64_OFFSET_BASIS;

    for byte in domain
        .as_bytes()
        .iter()
        .copied()
        .chain([0])
        .chain(payload.as_bytes().iter().copied())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV1A64_PRIME);
    }

    format!("{DIGEST_ALGORITHM}:{hash:016x}")
}

fn to_u64(value: usize) -> u64 {
    value.try_into().unwrap_or(u64::MAX)
}

fn div_ceil(value: u64, divisor: u64) -> u64 {
    if value == 0 {
        0
    } else {
        1 + ((value - 1) / divisor)
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct PayloadFeatures {
    kind: PayloadKind,
    char_count: u64,
    line_count: u64,
    whitespace_count: u64,
    word_runs: u64,
    number_runs: u64,
    punctuation_count: u64,
    symbol_count: u64,
    operator_count: u64,
    non_ascii_bytes: u64,
}

impl PayloadFeatures {
    fn analyze(payload: &str) -> Self {
        if payload.is_empty() {
            return Self {
                kind: PayloadKind::Empty,
                char_count: 0,
                line_count: 0,
                whitespace_count: 0,
                word_runs: 0,
                number_runs: 0,
                punctuation_count: 0,
                symbol_count: 0,
                operator_count: 0,
                non_ascii_bytes: 0,
            };
        }

        let mut features = Self {
            kind: PayloadKind::Text,
            char_count: 0,
            line_count: 1,
            whitespace_count: 0,
            word_runs: 0,
            number_runs: 0,
            punctuation_count: 0,
            symbol_count: 0,
            operator_count: 0,
            non_ascii_bytes: 0,
        };
        let mut previous_class = CharClass::Other;
        let mut code_score = 0u64;
        let mut structured_score = 0u64;

        for ch in payload.chars() {
            features.char_count = features.char_count.saturating_add(1);

            if ch == '\n' {
                features.line_count = features.line_count.saturating_add(1);
            }

            if ch.is_whitespace() {
                features.whitespace_count = features.whitespace_count.saturating_add(1);
            }

            if !ch.is_ascii() {
                features.non_ascii_bytes = features
                    .non_ascii_bytes
                    .saturating_add(to_u64(ch.len_utf8()));
            }

            let class = classify_char(ch);
            match class {
                CharClass::Word if previous_class != CharClass::Word => {
                    features.word_runs = features.word_runs.saturating_add(1);
                }
                CharClass::Number if previous_class != CharClass::Number => {
                    features.number_runs = features.number_runs.saturating_add(1);
                }
                CharClass::Punctuation => {
                    features.punctuation_count = features.punctuation_count.saturating_add(1);
                }
                CharClass::Symbol => {
                    features.symbol_count = features.symbol_count.saturating_add(1);
                    if is_operator_char(ch) {
                        features.operator_count = features.operator_count.saturating_add(1);
                    }
                    if matches!(ch, '{' | '}' | '[' | ']' | ':' | ',') {
                        structured_score = structured_score.saturating_add(1);
                    }
                    if is_code_signal(ch) {
                        code_score = code_score.saturating_add(1);
                    }
                }
                CharClass::Whitespace | CharClass::Other => {}
                CharClass::Word | CharClass::Number => {}
            }
            previous_class = class;
        }

        for marker in [
            "fn ", "let ", "const ", "class ", "def ", "impl ", "return ", "=>", "&&", "||", "::",
            "();",
        ] {
            if payload.contains(marker) {
                code_score = code_score.saturating_add(3);
            }
        }

        let trimmed = payload.trim_start();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            structured_score = structured_score.saturating_add(5);
        }

        features.kind = if code_score >= 5 {
            PayloadKind::Code
        } else if structured_score >= 6 {
            PayloadKind::Structured
        } else {
            PayloadKind::Text
        };

        features
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum CharClass {
    Whitespace,
    Word,
    Number,
    Punctuation,
    Symbol,
    Other,
}

fn classify_char(ch: char) -> CharClass {
    if ch.is_whitespace() {
        CharClass::Whitespace
    } else if ch.is_ascii_alphabetic() || ch == '_' {
        CharClass::Word
    } else if ch.is_ascii_digit() {
        CharClass::Number
    } else if ch.is_ascii_punctuation() {
        if is_symbol_char(ch) {
            CharClass::Symbol
        } else {
            CharClass::Punctuation
        }
    } else {
        CharClass::Other
    }
}

fn is_symbol_char(ch: char) -> bool {
    matches!(
        ch,
        '{' | '}'
            | '['
            | ']'
            | '('
            | ')'
            | '<'
            | '>'
            | '='
            | '+'
            | '-'
            | '*'
            | '/'
            | '\\'
            | '|'
            | '&'
            | ':'
            | ';'
            | ','
            | '.'
    )
}

fn is_operator_char(ch: char) -> bool {
    matches!(
        ch,
        '=' | '+' | '-' | '*' | '/' | '\\' | '|' | '&' | '<' | '>' | '!' | '%' | '^'
    )
}

fn is_code_signal(ch: char) -> bool {
    matches!(ch, '{' | '}' | '(' | ')' | ';' | '=' | '<' | '>')
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAW_MARKER: &str = "SECRET_CUSTOMER_SOURCE_SHOULD_NOT_PERSIST";

    #[test]
    fn deterministic_estimates_for_stable_fixture() {
        let payload = "build completed\nwarning: unused variable\n";

        let first = estimate_tool_result_tokens(payload);
        let second = estimate_tool_result_tokens(payload);

        assert_eq!(first, second);
        assert_eq!(first.tokens.output_tokens, 10);
        assert_eq!(first.raw_estimated_tokens, 10);
        assert_eq!(first.metadata.payload_kind, PayloadKind::Text);
        assert_eq!(first.content_digest, "fnv1a64:076568f6000509aa");
    }

    #[test]
    fn code_and_text_payloads_estimate_differently() {
        let text = "Create a helper that returns the current status for every configured task.";
        let code = "pub fn status(tasks: &[Task]) -> Vec<Status> {\n    tasks.iter().map(Status::from).collect()\n}\n";

        let text_estimate = estimate_tool_result_tokens(text);
        let code_estimate = estimate_tool_result_tokens(code);

        assert_eq!(text_estimate.metadata.payload_kind, PayloadKind::Text);
        assert_eq!(code_estimate.metadata.payload_kind, PayloadKind::Code);
        assert!(code_estimate.tokens.output_tokens > text_estimate.tokens.output_tokens);
    }

    #[test]
    fn empty_payloads_have_zero_tokens_and_empty_metadata_shape() {
        let estimate = estimate_tool_result_tokens("");

        assert_eq!(estimate.tokens, EstimatedTokenCounts::default());
        assert_eq!(estimate.raw_estimated_tokens, 0);
        assert_eq!(estimate.metadata.payload_kind, PayloadKind::Empty);
        assert_eq!(estimate.metadata.byte_count, 0);
        assert_eq!(estimate.metadata.char_count, 0);
        assert_eq!(estimate.metadata.line_count, 0);
        assert!(estimate.metadata.estimated);
    }

    #[test]
    fn metadata_clearly_marks_estimates_and_strategy() {
        let estimate = estimate_tool_result_tokens("plain output");
        let pairs = estimate.metadata_pairs();

        assert!(pairs.contains(&("estimated", "true".to_owned())));
        assert!(pairs.contains(&("strategy", TOKENIZER_STRATEGY.to_owned())));
        assert!(pairs.contains(&("tokenizer", TOKENIZER_LABEL.to_owned())));
        assert!(pairs.contains(&("provider_specific", "false".to_owned())));
        assert!(pairs.contains(&("content_digest", estimate.content_digest.clone())));
    }

    #[test]
    fn calibration_uses_aggregate_reported_to_estimated_factor() {
        let samples = [
            CalibrationSample {
                estimated_tokens: 100,
                reported_tokens: 125,
            },
            CalibrationSample {
                estimated_tokens: 60,
                reported_tokens: 75,
            },
        ];
        let factor = calibration_from_samples(&samples).expect("factor");
        let estimate = estimate_tool_result_tokens_with_config(
            "build completed\nwarning: unused variable\n",
            TokenEstimatorConfig::calibrated(factor),
        );

        assert_eq!(factor.numerator, 200);
        assert_eq!(factor.denominator, 160);
        assert_eq!(factor.sample_count, 2);
        assert_eq!(estimate.raw_estimated_tokens, 10);
        assert_eq!(estimate.tokens.output_tokens, 13);
        assert_eq!(
            estimate.metadata.calibration.unwrap().method,
            CALIBRATION_LABEL
        );
        assert!(estimate.metadata.estimated);
    }

    #[test]
    fn debug_and_metadata_do_not_persist_raw_content() {
        let payload =
            format!("tool output includes {RAW_MARKER} and private path /tmp/customer.rs");
        let estimate = estimate_tool_result_tokens(&payload);

        let debug = format!("{estimate:?}");
        let metadata = format!("{:?}", estimate.metadata_pairs());

        assert!(!debug.contains(RAW_MARKER));
        assert!(!metadata.contains(RAW_MARKER));
        assert!(!debug.contains("/tmp/customer.rs"));
        assert!(!metadata.contains("/tmp/customer.rs"));
        assert!(!estimate.content_digest.contains(RAW_MARKER));
    }
}
