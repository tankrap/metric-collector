# Metric Taker design decisions

## edit.echo reporting rule

`edit.echo` is reported as a separate operation class and is not folded into
the headline version-control share. Reports may show a combined file-interaction
view for exploration, but the headline VC/file-interaction claim keeps
`edit.echo` visible as editor/tool echo cost.

Rationale: edit echo is usually caused by editor-tool design rather than
version-control behavior. Keeping it separate makes comparisons with external
telemetry taxonomies clearer and prevents write-heavy sessions from overstating
VC cost.

## Comparison repetition and dispersion

TOPEN-3 is scoped to Mode T only. Mode T comparisons require at least two
repetitions per task/profile. Reports show medians and IQR when repeated
measurements exist, and they always display completion rates next to token
totals.

Rationale: small tester samples need uncertainty surfaced without making the
five-minute onboarding path depend on a large benchmark campaign.

## Passive self-reports

TOPEN-5 adds lightweight passive self-reports. `vc-tokmeter status` is the
first self-report surface: it prints the current mode, task/profile labels,
events captured today, and the top operation class without raw content.

Rationale: passive mode is the product path, so testers need a low-friction way
to confirm local capture is alive before report generation exists. This should
stay privacy-safe enough to paste into an issue or chat.

## Anonymous aggregation

Anonymous aggregation is out of v1. v1 has no telemetry phone-home; sharing is
manual and explicit via `vc-tokmeter report --share`.

Future opt-in aggregation can reuse the redacted share artifact boundary, but
the local event schema does not reserve remote upload fields in v1.
