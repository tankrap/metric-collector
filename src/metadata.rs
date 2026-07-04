use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct RunSettingsMetadata {
    pub model: Option<String>,
    pub provider: Option<String>,
    pub adapter: Option<String>,
    pub profile: Option<String>,
    pub relevant_settings: BTreeMap<String, String>,
}

impl RunSettingsMetadata {
    pub fn new<I, K, V>(
        model: Option<&str>,
        provider: Option<&str>,
        adapter: Option<&str>,
        profile: Option<&str>,
        relevant_settings: I,
    ) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        Self {
            model: model.map(str::to_owned),
            provider: provider.map(str::to_owned),
            adapter: adapter.map(str::to_owned),
            profile: profile.map(str::to_owned),
            relevant_settings: relevant_settings
                .into_iter()
                .map(|(key, value)| (key.into(), value.into()))
                .collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MetadataSide {
    Baseline,
    Treatment,
}

impl MetadataSide {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::Treatment => "treatment",
        }
    }
}

impl fmt::Display for MetadataSide {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MismatchWarning {
    MissingRunSettings {
        side: MetadataSide,
    },
    MissingField {
        side: MetadataSide,
        field: String,
    },
    FieldMismatch {
        field: String,
        baseline: String,
        treatment: String,
    },
    MissingSetting {
        side: MetadataSide,
        key: String,
    },
    SettingMismatch {
        key: String,
        baseline: String,
        treatment: String,
    },
}

impl MismatchWarning {
    pub fn to_report_warning(&self) -> String {
        match self {
            Self::MissingRunSettings { side } => {
                format!("missing {side} run settings metadata")
            }
            Self::MissingField { side, field } => {
                format!("missing {side} metadata field '{field}'")
            }
            Self::FieldMismatch {
                field,
                baseline,
                treatment,
            } => format!(
                "metadata mismatch for {field}: baseline='{baseline}', treatment='{treatment}'"
            ),
            Self::MissingSetting { side, key } => {
                format!("missing {side} metadata setting '{key}'")
            }
            Self::SettingMismatch {
                key,
                baseline,
                treatment,
            } => format!(
                "metadata setting mismatch for {key}: baseline='{baseline}', treatment='{treatment}'"
            ),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MetadataDiff {
    pub warnings: Vec<MismatchWarning>,
}

impl MetadataDiff {
    pub fn compare(
        baseline: Option<&RunSettingsMetadata>,
        treatment: Option<&RunSettingsMetadata>,
    ) -> Self {
        let mut diff = Self::default();

        match (baseline, treatment) {
            (Some(baseline), Some(treatment)) => {
                diff.compare_field("model", &baseline.model, &treatment.model);
                diff.compare_field("provider", &baseline.provider, &treatment.provider);
                diff.compare_field("adapter", &baseline.adapter, &treatment.adapter);
                diff.compare_field("profile", &baseline.profile, &treatment.profile);
                diff.compare_settings(&baseline.relevant_settings, &treatment.relevant_settings);
            }
            (None, Some(_)) => diff.warnings.push(MismatchWarning::MissingRunSettings {
                side: MetadataSide::Baseline,
            }),
            (Some(_), None) => diff.warnings.push(MismatchWarning::MissingRunSettings {
                side: MetadataSide::Treatment,
            }),
            (None, None) => {
                diff.warnings.push(MismatchWarning::MissingRunSettings {
                    side: MetadataSide::Baseline,
                });
                diff.warnings.push(MismatchWarning::MissingRunSettings {
                    side: MetadataSide::Treatment,
                });
            }
        }

        diff
    }

    pub fn is_empty(&self) -> bool {
        self.warnings.is_empty()
    }

    pub fn report_warnings(&self) -> Vec<String> {
        self.warnings
            .iter()
            .map(MismatchWarning::to_report_warning)
            .collect()
    }

    fn compare_field(
        &mut self,
        field: &'static str,
        baseline: &Option<String>,
        treatment: &Option<String>,
    ) {
        match (baseline, treatment) {
            (Some(baseline), Some(treatment)) if baseline != treatment => {
                self.warnings.push(MismatchWarning::FieldMismatch {
                    field: field.to_owned(),
                    baseline: baseline.clone(),
                    treatment: treatment.clone(),
                });
            }
            (None, Some(_)) => self.warnings.push(MismatchWarning::MissingField {
                side: MetadataSide::Baseline,
                field: field.to_owned(),
            }),
            (Some(_), None) => self.warnings.push(MismatchWarning::MissingField {
                side: MetadataSide::Treatment,
                field: field.to_owned(),
            }),
            (None, None) => {
                self.warnings.push(MismatchWarning::MissingField {
                    side: MetadataSide::Baseline,
                    field: field.to_owned(),
                });
                self.warnings.push(MismatchWarning::MissingField {
                    side: MetadataSide::Treatment,
                    field: field.to_owned(),
                });
            }
            _ => {}
        }
    }

    fn compare_settings(
        &mut self,
        baseline: &BTreeMap<String, String>,
        treatment: &BTreeMap<String, String>,
    ) {
        let keys: BTreeSet<&String> = baseline.keys().chain(treatment.keys()).collect();

        for key in keys {
            match (baseline.get(key), treatment.get(key)) {
                (Some(baseline), Some(treatment)) if baseline != treatment => {
                    self.warnings.push(MismatchWarning::SettingMismatch {
                        key: key.clone(),
                        baseline: baseline.clone(),
                        treatment: treatment.clone(),
                    });
                }
                (None, Some(_)) => self.warnings.push(MismatchWarning::MissingSetting {
                    side: MetadataSide::Baseline,
                    key: key.clone(),
                }),
                (Some(_), None) => self.warnings.push(MismatchWarning::MissingSetting {
                    side: MetadataSide::Treatment,
                    key: key.clone(),
                }),
                _ => {}
            }
        }
    }
}

pub fn metadata_warnings_for_report(
    baseline: Option<&RunSettingsMetadata>,
    treatment: Option<&RunSettingsMetadata>,
) -> Vec<String> {
    MetadataDiff::compare(baseline, treatment).report_warnings()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metadata(
        model: Option<&str>,
        provider: Option<&str>,
        adapter: Option<&str>,
        profile: Option<&str>,
        settings: &[(&str, &str)],
    ) -> RunSettingsMetadata {
        RunSettingsMetadata::new(model, provider, adapter, profile, settings.iter().copied())
    }

    #[test]
    fn equal_metadata_has_no_warnings() {
        let baseline = metadata(
            Some("gpt-5"),
            Some("openai"),
            Some("codex-cli"),
            Some("baseline"),
            &[("reasoning_effort", "medium"), ("temperature", "0")],
        );
        let treatment = baseline.clone();

        let diff = MetadataDiff::compare(Some(&baseline), Some(&treatment));

        assert!(diff.is_empty());
        assert!(diff.report_warnings().is_empty());
    }

    #[test]
    fn records_model_mismatch() {
        let baseline = metadata(
            Some("gpt-5"),
            Some("openai"),
            Some("codex-cli"),
            Some("baseline"),
            &[("reasoning_effort", "medium")],
        );
        let treatment = metadata(
            Some("gpt-5-mini"),
            Some("openai"),
            Some("codex-cli"),
            Some("baseline"),
            &[("reasoning_effort", "medium")],
        );

        let diff = MetadataDiff::compare(Some(&baseline), Some(&treatment));

        assert_eq!(
            diff.warnings,
            vec![MismatchWarning::FieldMismatch {
                field: "model".to_owned(),
                baseline: "gpt-5".to_owned(),
                treatment: "gpt-5-mini".to_owned(),
            }]
        );
        assert_eq!(
            diff.report_warnings(),
            vec!["metadata mismatch for model: baseline='gpt-5', treatment='gpt-5-mini'"]
        );
    }

    #[test]
    fn records_setting_mismatch_in_sorted_key_order() {
        let baseline = metadata(
            Some("gpt-5"),
            Some("openai"),
            Some("codex-cli"),
            Some("baseline"),
            &[("temperature", "0"), ("reasoning_effort", "low")],
        );
        let treatment = metadata(
            Some("gpt-5"),
            Some("openai"),
            Some("codex-cli"),
            Some("baseline"),
            &[("temperature", "0"), ("reasoning_effort", "high")],
        );

        let diff = MetadataDiff::compare(Some(&baseline), Some(&treatment));

        assert_eq!(
            diff.warnings,
            vec![MismatchWarning::SettingMismatch {
                key: "reasoning_effort".to_owned(),
                baseline: "low".to_owned(),
                treatment: "high".to_owned(),
            }]
        );
        assert_eq!(
            diff.report_warnings(),
            vec![
                "metadata setting mismatch for reasoning_effort: baseline='low', treatment='high'"
            ]
        );
    }

    #[test]
    fn records_missing_metadata_warning_inputs_for_report_generation() {
        let baseline = metadata(
            None,
            Some("openai"),
            Some("codex-cli"),
            Some("baseline"),
            &[("reasoning_effort", "medium")],
        );
        let treatment = metadata(
            Some("gpt-5"),
            Some("openai"),
            Some("codex-cli"),
            None,
            &[("temperature", "0")],
        );

        let warnings = metadata_warnings_for_report(Some(&baseline), Some(&treatment));

        assert_eq!(
            warnings,
            vec![
                "missing baseline metadata field 'model'",
                "missing treatment metadata field 'profile'",
                "missing treatment metadata setting 'reasoning_effort'",
                "missing baseline metadata setting 'temperature'",
            ]
        );
    }

    #[test]
    fn records_missing_whole_run_metadata_for_each_side() {
        assert_eq!(
            metadata_warnings_for_report(None, None),
            vec![
                "missing baseline run settings metadata",
                "missing treatment run settings metadata",
            ]
        );
    }
}
