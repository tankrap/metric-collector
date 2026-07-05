#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
import urllib.error
import urllib.request


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Verify the deployed collector dashboard HTML is nonblank."
    )
    parser.add_argument(
        "--base-url",
        default=os.environ.get("COLLECTOR_BASE_URL", "http://127.0.0.1:8088"),
    )
    parser.add_argument("--timeout", type=float, default=5.0)
    args = parser.parse_args()

    status, content_type, body = get(args.base_url.rstrip("/") + "/dashboard", args.timeout)
    if status != 200:
        raise AssertionError(f"/dashboard returned {status}")
    if "text/html" not in content_type:
        raise AssertionError(f"/dashboard content-type was {content_type!r}, not text/html")
    if len(body.strip()) < 500:
        raise AssertionError("/dashboard HTML was unexpectedly small")
    required = ["Metric Taker Dashboard", "Admin token", "/v1/dashboard/summary"]
    missing = [text for text in required if text not in body]
    if missing:
        raise AssertionError(f"/dashboard HTML missing expected markers: {missing}")

    print(f"collector dashboard smoke passed: {args.base_url.rstrip('/')}/dashboard")
    return 0


def get(url: str, timeout: float) -> tuple[int, str, str]:
    req = urllib.request.Request(url, method="GET")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as response:
            return (
                response.status,
                response.headers.get("content-type", ""),
                response.read().decode("utf-8", errors="replace"),
            )
    except urllib.error.HTTPError as exc:
        return (
            exc.status,
            exc.headers.get("content-type", ""),
            exc.read().decode("utf-8", errors="replace"),
        )


if __name__ == "__main__":
    raise SystemExit(main())
