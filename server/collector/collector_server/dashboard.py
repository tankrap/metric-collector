from __future__ import annotations

from collections import Counter, defaultdict
from datetime import datetime, timedelta, timezone
from html import escape
from typing import Any, Iterable

from .storage import UploadRecord

DASHBOARD_SCHEMA_VERSION = 1
WINDOWS = {
    "all": None,
    "7d": timedelta(days=7),
    "30d": timedelta(days=30),
}


def dashboard_summary(
    records: Iterable[UploadRecord],
    *,
    window: str = "all",
    now: datetime | None = None,
) -> dict[str, Any]:
    selected = _filter_window(records, window=window, now=now)
    totals = _totals(selected)
    latest = max(selected, key=lambda record: record.created_at, default=None)

    return {
        "schema_version": DASHBOARD_SCHEMA_VERSION,
        "window": _normalize_window(window),
        "upload_count": len(selected),
        "total_tokens": totals["total_tokens"],
        "git_tokens": totals["git_tokens"],
        "non_git_tokens": totals["total_tokens"] - totals["git_tokens"],
        "git_token_share": _share(totals["git_tokens"], totals["total_tokens"]),
        "average_git_token_share": _average(
            record.git_token_share for record in selected
        ),
        "latest_upload": _latest_upload(latest),
    }


def dashboard_time_series(
    records: Iterable[UploadRecord],
    *,
    window: str = "all",
    now: datetime | None = None,
) -> dict[str, Any]:
    selected = _filter_window(records, window=window, now=now)
    buckets: dict[str, dict[str, int]] = defaultdict(
        lambda: {"upload_count": 0, "total_tokens": 0, "git_tokens": 0}
    )

    for record in selected:
        bucket = _parse_datetime(record.created_at).date().isoformat()
        buckets[bucket]["upload_count"] += 1
        buckets[bucket]["total_tokens"] += record.total_tokens
        buckets[bucket]["git_tokens"] += record.git_tokens

    rows = []
    for bucket in sorted(buckets):
        row = buckets[bucket]
        rows.append(
            {
                "bucket": bucket,
                "upload_count": row["upload_count"],
                "total_tokens": row["total_tokens"],
                "git_tokens": row["git_tokens"],
                "git_token_share": _share(row["git_tokens"], row["total_tokens"]),
            }
        )

    return {
        "schema_version": DASHBOARD_SCHEMA_VERSION,
        "window": _normalize_window(window),
        "bucket": "day",
        "rows": rows,
    }


def dashboard_surface_distribution(
    records: Iterable[UploadRecord],
    *,
    window: str = "all",
    now: datetime | None = None,
) -> dict[str, Any]:
    return _dimension_distribution(
        _filter_window(records, window=window, now=now),
        window=window,
        dimension="surface",
    )


def dashboard_fidelity_distribution(
    records: Iterable[UploadRecord],
    *,
    window: str = "all",
    now: datetime | None = None,
) -> dict[str, Any]:
    return _dimension_distribution(
        _filter_window(records, window=window, now=now),
        window=window,
        dimension="fidelity",
    )


def dashboard_top_git_actions(
    records: Iterable[UploadRecord],
    *,
    window: str = "all",
    now: datetime | None = None,
    limit: int = 10,
) -> dict[str, Any]:
    selected = _filter_window(records, window=window, now=now)
    totals = _totals(selected)
    actions: dict[str, dict[str, int]] = defaultdict(
        lambda: {"event_count": 0, "total_tokens": 0, "input_tokens": 0, "output_tokens": 0}
    )

    for record in selected:
        for row in _git_workflow_rows(record.payload):
            action = _short_string(row.get("action_subtype"), default="unknown")
            row_totals = row.get("totals") if isinstance(row.get("totals"), dict) else row
            actions[action]["event_count"] += _int_value(row_totals, "events")
            actions[action]["event_count"] += _int_value(row_totals, "event_count")
            actions[action]["total_tokens"] += _int_value(row_totals, "total_tokens")
            actions[action]["input_tokens"] += _int_value(row_totals, "input_tokens")
            actions[action]["output_tokens"] += _int_value(row_totals, "output_tokens")

    safe_limit = max(1, min(limit, 50))
    rows = []
    for action, action_totals in sorted(
        actions.items(),
        key=lambda item: (-item[1]["total_tokens"], item[0]),
    )[:safe_limit]:
        rows.append(
            {
                "action_subtype": action,
                "event_count": action_totals["event_count"],
                "total_tokens": action_totals["total_tokens"],
                "input_tokens": action_totals["input_tokens"],
                "output_tokens": action_totals["output_tokens"],
                "share_of_all_tokens": _share(
                    action_totals["total_tokens"], totals["total_tokens"]
                ),
            }
        )

    return {
        "schema_version": DASHBOARD_SCHEMA_VERSION,
        "window": _normalize_window(window),
        "rows": rows,
    }


def dashboard_page_model(
    records: Iterable[UploadRecord],
    *,
    window: str = "all",
) -> dict[str, Any]:
    selected = list(records)
    return {
        "window": _normalize_window(window),
        "summary": dashboard_summary(selected, window=window),
        "time_series": dashboard_time_series(selected, window=window),
        "surfaces": dashboard_surface_distribution(selected, window=window),
        "fidelity": dashboard_fidelity_distribution(selected, window=window),
        "git_actions": dashboard_top_git_actions(selected, window=window, limit=10),
    }


def render_dashboard_html(model: dict[str, Any]) -> str:
    window = str(model["window"])
    summary = model["summary"]
    latest = summary.get("latest_upload")
    export_json = "/v1/uploads/export"
    export_ndjson = "/v1/uploads/export?format=ndjson"

    return f"""<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Metric Taker Dashboard</title>
  <style>
    :root {{
      color-scheme: light;
      --bg: #f7f8fa;
      --panel: #ffffff;
      --text: #1f2933;
      --muted: #5f6b7a;
      --line: #d9dee7;
      --accent: #0072c3;
      --accent-2: #2e7d32;
    }}
    * {{ box-sizing: border-box; }}
    body {{
      margin: 0;
      background: var(--bg);
      color: var(--text);
      font: 14px/1.5 -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }}
    header {{
      border-bottom: 1px solid var(--line);
      background: var(--panel);
    }}
    main, .topbar {{
      width: min(1180px, calc(100% - 32px));
      margin: 0 auto;
    }}
    .topbar {{
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 16px;
      padding: 20px 0;
    }}
    h1 {{ margin: 0; font-size: 22px; font-weight: 650; }}
    h2 {{ margin: 0 0 12px; font-size: 16px; font-weight: 650; }}
    main {{ padding: 24px 0 40px; }}
    .controls, .links {{ display: flex; gap: 8px; flex-wrap: wrap; }}
    a.button {{
      display: inline-flex;
      align-items: center;
      min-height: 34px;
      padding: 0 12px;
      border: 1px solid var(--line);
      border-radius: 6px;
      background: var(--panel);
      color: var(--text);
      text-decoration: none;
      font-weight: 550;
    }}
    a.button.active {{ border-color: var(--accent); color: var(--accent); }}
    .cards {{
      display: grid;
      grid-template-columns: repeat(5, minmax(150px, 1fr));
      gap: 12px;
      margin-bottom: 18px;
    }}
    .card, section {{
      background: var(--panel);
      border: 1px solid var(--line);
      border-radius: 8px;
    }}
    .card {{ padding: 14px; min-height: 96px; }}
    .label {{ color: var(--muted); font-size: 12px; font-weight: 600; text-transform: uppercase; }}
    .value {{ margin-top: 8px; font-size: 26px; font-weight: 700; }}
    .subtle {{ color: var(--muted); }}
    .grid {{
      display: grid;
      grid-template-columns: 1.2fr .8fr;
      gap: 16px;
      align-items: start;
    }}
    section {{ padding: 16px; margin-bottom: 16px; }}
    table {{ width: 100%; border-collapse: collapse; }}
    th, td {{
      padding: 9px 8px;
      border-bottom: 1px solid var(--line);
      text-align: right;
      vertical-align: middle;
    }}
    th:first-child, td:first-child {{ text-align: left; }}
    tr:last-child td {{ border-bottom: 0; }}
    th {{ color: var(--muted); font-size: 12px; font-weight: 650; }}
    .bar {{
      height: 10px;
      min-width: 2px;
      border-radius: 999px;
      background: var(--accent);
    }}
    .bar.secondary {{ background: var(--accent-2); }}
    .bar-cell {{ width: 34%; }}
    .empty {{ padding: 18px 0; color: var(--muted); }}
    @media (max-width: 900px) {{
      .topbar {{ align-items: flex-start; flex-direction: column; }}
      .cards {{ grid-template-columns: repeat(2, minmax(0, 1fr)); }}
      .grid {{ grid-template-columns: 1fr; }}
    }}
    @media (max-width: 520px) {{
      main, .topbar {{ width: min(100% - 20px, 1180px); }}
      .cards {{ grid-template-columns: 1fr; }}
      th, td {{ padding: 8px 4px; }}
    }}
  </style>
</head>
<body>
  <header>
    <div class="topbar">
      <div>
        <h1>Metric Taker Dashboard</h1>
        <div class="subtle">Aggregate study metrics only</div>
      </div>
      <nav class="controls" aria-label="Dashboard window">
        {_window_link("all", window)}
        {_window_link("7d", window)}
        {_window_link("30d", window)}
      </nav>
    </div>
  </header>
  <main>
    <div class="cards">
      {_card("Uploads", _format_int(summary["upload_count"]))}
      {_card("Total Tokens", _format_int(summary["total_tokens"]))}
      {_card("Git Tokens", _format_int(summary["git_tokens"]))}
      {_card("Git Share", _format_percent(summary["git_token_share"]))}
      {_card("Avg Git Share", _format_percent(summary["average_git_token_share"]))}
    </div>

    <section>
      <h2>Admin Export</h2>
      <div class="links">
        <a class="button" href="{export_json}">Download JSON</a>
        <a class="button" href="{export_ndjson}">Download NDJSON</a>
      </div>
    </section>

    <div class="grid">
      <section>
        <h2>Daily Volume</h2>
        {_time_series_table(model["time_series"]["rows"])}
      </section>
      <section>
        <h2>Latest Upload Metadata</h2>
        {_latest_upload_table(latest)}
      </section>
    </div>

    <div class="grid">
      <section>
        <h2>Surfaces</h2>
        {_distribution_table(model["surfaces"]["rows"], "surface", "Surface")}
      </section>
      <section>
        <h2>Fidelity</h2>
        {_distribution_table(model["fidelity"]["rows"], "fidelity", "Fidelity")}
      </section>
    </div>

    <section>
      <h2>Top Git Actions</h2>
      {_git_actions_table(model["git_actions"]["rows"])}
    </section>
  </main>
</body>
</html>
"""


def _window_link(value: str, active: str) -> str:
    label = {"all": "All", "7d": "7 days", "30d": "30 days"}[value]
    href = "/dashboard" if value == "all" else f"/dashboard?window={value}"
    classes = "button active" if value == active else "button"
    return f'<a class="{classes}" href="{href}">{label}</a>'


def _card(label: str, value: str) -> str:
    return (
        '<div class="card">'
        f'<div class="label">{escape(label)}</div>'
        f'<div class="value">{escape(value)}</div>'
        "</div>"
    )


def _latest_upload_table(value: dict[str, Any] | None) -> str:
    if not value:
        return '<div class="empty">No uploads in this window.</div>'
    rows = [
        ("Created", _text(value.get("created_at"))),
        ("Surface", _text(value.get("surface"))),
        ("Fidelity", _text(value.get("fidelity"))),
    ]
    cells = "".join(
        f"<tr><th>{escape(label)}</th><td>{escape(child)}</td></tr>"
        for label, child in rows
    )
    return f"<table><tbody>{cells}</tbody></table>"


def _time_series_table(rows: list[dict[str, Any]]) -> str:
    if not rows:
        return '<div class="empty">No daily volume yet.</div>'
    max_tokens = max((row["total_tokens"] for row in rows), default=0)
    body = "".join(
        "<tr>"
        f"<td>{escape(_text(row['bucket']))}</td>"
        f"<td>{_format_int(row['upload_count'])}</td>"
        f"<td>{_format_int(row['total_tokens'])}</td>"
        f"<td>{_format_percent(row['git_token_share'])}</td>"
        f'<td class="bar-cell"><div class="bar" style="width:{_bar_width(row["total_tokens"], max_tokens)}%"></div></td>'
        "</tr>"
        for row in rows
    )
    return (
        "<table><thead><tr><th>Day</th><th>Uploads</th><th>Total Tokens</th>"
        "<th>Git Share</th><th>Volume</th></tr></thead>"
        f"<tbody>{body}</tbody></table>"
    )


def _distribution_table(rows: list[dict[str, Any]], key: str, label: str) -> str:
    if not rows:
        return '<div class="empty">No distribution data yet.</div>'
    body = "".join(
        "<tr>"
        f"<td>{escape(_text(row[key]))}</td>"
        f"<td>{_format_int(row['upload_count'])}</td>"
        f"<td>{_format_int(row['total_tokens'])}</td>"
        f"<td>{_format_percent(row['git_token_share'])}</td>"
        f'<td class="bar-cell"><div class="bar secondary" style="width:{_bar_width(row["share_of_all_tokens"], 1.0)}%"></div></td>'
        "</tr>"
        for row in rows
    )
    return (
        f"<table><thead><tr><th>{escape(label)}</th><th>Uploads</th>"
        "<th>Total Tokens</th><th>Git Share</th><th>Token Share</th></tr></thead>"
        f"<tbody>{body}</tbody></table>"
    )


def _git_actions_table(rows: list[dict[str, Any]]) -> str:
    if not rows:
        return '<div class="empty">No git action data yet.</div>'
    body = "".join(
        "<tr>"
        f"<td>{escape(_text(row['action_subtype']))}</td>"
        f"<td>{_format_int(row['event_count'])}</td>"
        f"<td>{_format_int(row['total_tokens'])}</td>"
        f"<td>{_format_int(row['input_tokens'])}</td>"
        f"<td>{_format_int(row['output_tokens'])}</td>"
        f"<td>{_format_percent(row['share_of_all_tokens'])}</td>"
        "</tr>"
        for row in rows
    )
    return (
        "<table><thead><tr><th>Action</th><th>Events</th><th>Total Tokens</th>"
        "<th>Input</th><th>Output</th><th>All Token Share</th></tr></thead>"
        f"<tbody>{body}</tbody></table>"
    )


def _bar_width(value: int | float, max_value: int | float) -> int:
    if not max_value:
        return 0
    return max(2, round((float(value) / float(max_value)) * 100))


def _format_int(value: int) -> str:
    return f"{value:,}"


def _format_percent(value: float) -> str:
    return f"{value * 100:.1f}%"


def _text(value: Any) -> str:
    return str(value) if value is not None else ""


def _dimension_distribution(
    records: list[UploadRecord],
    *,
    window: str,
    dimension: str,
) -> dict[str, Any]:
    totals = _totals(records)
    counts = Counter(getattr(record, dimension) for record in records)
    token_totals: dict[str, dict[str, int]] = defaultdict(
        lambda: {"total_tokens": 0, "git_tokens": 0}
    )

    for record in records:
        value = getattr(record, dimension)
        token_totals[value]["total_tokens"] += record.total_tokens
        token_totals[value]["git_tokens"] += record.git_tokens

    rows = []
    for value, count in sorted(counts.items(), key=lambda item: (-item[1], item[0])):
        row_totals = token_totals[value]
        rows.append(
            {
                dimension: value,
                "upload_count": count,
                "upload_share": _share(count, len(records)),
                "total_tokens": row_totals["total_tokens"],
                "git_tokens": row_totals["git_tokens"],
                "git_token_share": _share(
                    row_totals["git_tokens"], row_totals["total_tokens"]
                ),
                "share_of_all_tokens": _share(
                    row_totals["total_tokens"], totals["total_tokens"]
                ),
            }
        )

    return {
        "schema_version": DASHBOARD_SCHEMA_VERSION,
        "window": _normalize_window(window),
        "rows": rows,
    }


def _filter_window(
    records: Iterable[UploadRecord],
    *,
    window: str,
    now: datetime | None,
) -> list[UploadRecord]:
    normalized = _normalize_window(window)
    delta = WINDOWS[normalized]
    selected = list(records)
    if delta is None:
        return selected

    clock = now or datetime.now(timezone.utc)
    cutoff = _ensure_utc(clock) - delta
    return [
        record
        for record in selected
        if _parse_datetime(record.created_at) >= cutoff
    ]


def _normalize_window(window: str) -> str:
    return window if window in WINDOWS else "all"


def _totals(records: Iterable[UploadRecord]) -> dict[str, int]:
    total_tokens = 0
    git_tokens = 0
    for record in records:
        total_tokens += record.total_tokens
        git_tokens += record.git_tokens
    return {"total_tokens": total_tokens, "git_tokens": git_tokens}


def _latest_upload(record: UploadRecord | None) -> dict[str, Any] | None:
    if record is None:
        return None
    return {
        "created_at": record.created_at,
        "surface": record.surface,
        "fidelity": record.fidelity,
    }


def _git_workflow_rows(payload: dict[str, Any]) -> list[dict[str, Any]]:
    metrics = payload.get("metrics")
    if isinstance(metrics, dict):
        git_workflow = metrics.get("git_workflow")
        if not isinstance(git_workflow, dict):
            return []
        rows = git_workflow.get("action_subtypes")
        if not isinstance(rows, list):
            return []
        return [row for row in rows if isinstance(row, dict)]

    report = payload.get("report")
    if not isinstance(report, dict):
        return []
    git_workflow = report.get("git_workflow")
    if not isinstance(git_workflow, dict):
        return []
    rows = git_workflow.get("rows")
    if not isinstance(rows, list):
        return []
    return [row for row in rows if isinstance(row, dict)]


def _int_value(value: dict[str, Any], key: str) -> int:
    child = value.get(key, 0)
    if isinstance(child, bool) or not isinstance(child, int) or child < 0:
        return 0
    return child


def _short_string(value: Any, *, default: str) -> str:
    if not isinstance(value, str):
        return default
    return value[:128] or default


def _average(values: Iterable[float]) -> float:
    total = 0.0
    count = 0
    for value in values:
        total += value
        count += 1
    return total / count if count else 0.0


def _share(part: int | float, whole: int | float) -> float:
    return float(part) / float(whole) if whole else 0.0


def _parse_datetime(value: str) -> datetime:
    return _ensure_utc(datetime.fromisoformat(value.replace("Z", "+00:00")))


def _ensure_utc(value: datetime) -> datetime:
    if value.tzinfo is None:
        return value.replace(tzinfo=timezone.utc)
    return value.astimezone(timezone.utc)
