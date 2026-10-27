# /// script
# requires-python = ">=3.11"
# dependencies = ["google-auth>=2.30", "requests>=2.32"]
# ///
"""Report CleanScope's Google Play publishing state as JSON.

Two sources:
  * Android Publisher API v3 (service account) — tracks, release lifecycle
    states, store listing text, app details, testers.
  * The public Play policy-deadlines table — upcoming deadlines, flagged when
    they mention a permission the app's manifest declares.

The Play Console dashboard checklist, Policy status, App content declarations
and Inbox have no API; they are listed under `console_only_checks` so the
report says what it could not see.

Usage:
  uv run scripts/play_status.py [--key-file service-account-play.json] [--markdown]
  GOOGLE_PLAY_SERVICE_ACCOUNT_JSON='{...}' uv run scripts/play_status.py

Exit codes: 0 ok, 2 Play API unreachable/unauthorised (deadlines still reported).
"""

from __future__ import annotations

import argparse
import datetime as dt
import html
import json
import os
import re
import sys
from html.parser import HTMLParser
from pathlib import Path

import requests
from google.auth.transport.requests import AuthorizedSession
from google.oauth2 import service_account

PACKAGE = "com.cleanscope.app"
API = "https://androidpublisher.googleapis.com/androidpublisher/v3/applications"
SCOPE = "https://www.googleapis.com/auth/androidpublisher"
DEADLINES_URL = "https://support.google.com/googleplay/android-developer/table/12921780?hl=en-GB"
MANIFEST = Path(__file__).resolve().parent.parent / "src-tauri/gen/android/app/src/main/AndroidManifest.xml"

# Deadlines that apply to every app regardless of permissions.
ACCOUNT_WIDE_POLICIES = ("Play Console requirements", "Target API level")

CONSOLE_ONLY_CHECKS = [
    "Dashboard 'Finish setting up your app' checklist (store listing graphics, declarations)",
    "Policy status / policy issues",
    "App content declarations (Data safety, Target audience, Ads, Content rating)",
    "Closed-test production-access criteria (12 opted-in testers for 14 days)",
    "Inbox messages and policy emails",
]


# --- Play Developer API -----------------------------------------------------


def load_credentials(key_file: str) -> service_account.Credentials:
    raw = os.environ.get("GOOGLE_PLAY_SERVICE_ACCOUNT_JSON")
    info = json.loads(raw) if raw else json.loads(Path(key_file).read_text())
    return service_account.Credentials.from_service_account_info(info, scopes=[SCOPE])


def api_error(resp: requests.Response) -> str:
    try:
        err = resp.json()["error"]
        return f"HTTP {err.get('code')} {err.get('status')}: {err.get('message')}"
    except (ValueError, KeyError):
        return f"HTTP {resp.status_code}: {resp.text[:200]}"


def get_json(session: AuthorizedSession, url: str) -> dict:
    resp = session.get(url)
    if not resp.ok:
        raise RuntimeError(api_error(resp))
    # tracks/{track}/releases answers 204 with an empty body for a track with no releases
    return resp.json() if resp.content else {}


def collect_play_state(session: AuthorizedSession, package: str) -> dict:
    """Open a throwaway edit, read state, delete the edit (nothing is committed)."""
    base = f"{API}/{package}"
    resp = session.post(f"{base}/edits")
    if not resp.ok:
        raise RuntimeError(api_error(resp))
    edit = f"{base}/edits/{resp.json()['id']}"
    try:
        details = get_json(session, f"{edit}/details")
        listings = get_json(session, f"{edit}/listings").get("listings", [])
        tracks = get_json(session, f"{edit}/tracks").get("tracks", [])
        testers = {
            t["track"]: get_json(session, f"{edit}/testers/{t['track']}") for t in tracks if t["track"] != "production"
        }
    finally:
        session.delete(edit)

    # The edit's track list carries name/status/versionCodes; releases.list adds the review
    # lifecycle state but has a tight per-app quota, so call it only for non-empty tracks
    # and fall back to the edit's status when it is rate-limited.
    lifecycle, warnings = {}, []
    for t in tracks:
        if not t.get("releases"):
            continue
        try:
            for r in get_json(session, f"{base}/tracks/{t['track']}/releases").get("releases", []):
                lifecycle[(t["track"], r.get("releaseName"))] = r.get("releaseLifecycleState")
        except RuntimeError as exc:
            warnings.append(f"{t['track']}: lifecycle state unavailable ({exc})")
    return {
        "details": details,
        "listings": [{k: lst.get(k) for k in ("language", "title", "shortDescription")} for lst in listings],
        "tracks": [
            {
                "track": t["track"],
                "releases": [
                    {
                        "name": r.get("name"),
                        "status": r.get("status"),
                        "state": lifecycle.get((t["track"], r.get("name"))),
                        "version_codes": r.get("versionCodes", []),
                    }
                    for r in t.get("releases", [])
                ],
            }
            for t in tracks
        ],
        "testers": testers,
        "warnings": warnings,
    }


# --- Policy deadlines -------------------------------------------------------


class DeadlineTableParser(HTMLParser):
    """Collect each <tr data-category> row of the deadlines table as cell texts."""

    def __init__(self) -> None:
        super().__init__()
        self.rows: list[dict] = []
        self._row: dict | None = None
        self._cell: list[str] | None = None
        self._in_strong = False

    def handle_starttag(self, tag, attrs):
        a = dict(attrs)
        if tag == "tr" and "data-category" in a:
            self._row = {"category": a["data-category"], "cells": [], "title": ""}
        elif self._row is not None and tag == "td":
            self._cell = []
        elif self._cell is not None and tag in ("div", "br", "li"):
            self._cell.append("\n")
        if tag == "strong":
            self._in_strong = True

    def handle_endtag(self, tag):
        if tag == "strong":
            self._in_strong = False
        elif tag == "td" and self._row is not None and self._cell is not None:
            self._row["cells"].append(re.sub(r"[ \t]+", " ", "".join(self._cell)).strip())
            self._cell = None
        elif tag == "tr" and self._row is not None:
            self.rows.append(self._row)
            self._row = None

    def handle_data(self, data):
        if self._cell is not None:
            self._cell.append(data)
            if self._in_strong and self._row is not None and not self._row["title"]:
                self._row["title"] = data.strip()


def declared_permissions(manifest: Path) -> list[str]:
    text = manifest.read_text() if manifest.exists() else ""
    return re.findall(r'uses-permission\s+android:name="android\.permission\.([A-Z_]+)"', text)


def permission_subject(permission: str) -> str:
    """READ_CONTACTS -> CONTACTS, ACCESS_FINE_LOCATION -> LOCATION: the word policy text uses."""
    return re.sub(r"^((READ|WRITE|ACCESS|FINE|COARSE|POST|RECEIVE|SEND|MANAGE|BIND)_)+", "", permission)


def mentions(permission: str, haystack: str) -> bool:
    return re.search(rf"(?<![A-Z]){permission_subject(permission)}(?![A-Z])", haystack) is not None


def parse_deadlines(page: str, today: dt.date, permissions: list[str]) -> list[dict]:
    parser = DeadlineTableParser()
    parser.feed(page)
    out = []
    for row in parser.rows:
        if len(row["cells"]) < 2 or not re.fullmatch(r"\d{4}-\d{2}-\d{2}", row["cells"][0]):
            continue
        deadline = dt.date.fromisoformat(row["cells"][0])
        body = html.unescape(row["cells"][1])
        lines = [ln.strip() for ln in body.splitlines() if ln.strip()]
        announced = next((ln.split()[-1] for ln in lines if ln.startswith("Announced")), None)
        summary = " ".join(ln for ln in lines if ln != row["title"] and not ln.startswith("Announced"))
        haystack = body.upper().replace(" ", "_")
        out.append(
            {
                "deadline": deadline.isoformat(),
                "days_left": (deadline - today).days,
                "policy": row["title"],
                "summary": summary,
                "announced": announced,
                "mentions_declared_permission": sorted(p for p in permissions if mentions(p, haystack)),
                "account_wide": any(name.lower() in row["title"].lower() for name in ACCOUNT_WIDE_POLICIES),
            }
        )
    return sorted(out, key=lambda d: d["deadline"])


# --- Findings & output ------------------------------------------------------


def findings(play: dict | None, deadlines: list[dict]) -> list[dict]:
    found = []
    if play:
        if not play["details"].get("contactWebsite"):
            found.append({"severity": "info", "message": "Store settings: contact website is empty"})
        tracks = {t["track"]: t for t in play["tracks"]}
        if not any(t != "internal" and tracks[t]["releases"] for t in tracks):
            found.append(
                {
                    "severity": "action",
                    "message": "No closed-testing release yet; production access needs a 14-day closed test with 12 testers",
                }
            )
        for t in play["tracks"]:
            for r in t["releases"]:
                if r["state"] in (
                    "RELEASE_LIFECYCLE_STATE_NOT_APPROVED",
                    "RELEASE_LIFECYCLE_STATE_NOT_SENT_FOR_REVIEW",
                ):
                    found.append(
                        {"severity": "action", "message": f"{t['track']}: release {r['name']} is {r['state']}"}
                    )
    for d in deadlines:
        if d["days_left"] < 0:
            continue
        if d["mentions_declared_permission"]:
            found.append(
                {
                    "severity": "action",
                    "message": f"Deadline {d['deadline']} ({d['policy']}) mentions declared permission(s) {d['mentions_declared_permission']}",
                }
            )
        elif d["account_wide"]:
            found.append(
                {"severity": "review", "message": f"Deadline {d['deadline']} ({d['policy']}) applies to all apps"}
            )
    return found


def to_markdown(report: dict) -> str:
    lines = [f"## Google Play status — `{report['package']}`", "", f"Generated {report['generated_at']}", ""]
    play = report["play_api"]
    if play["error"]:
        lines += [f"**Play API error:** {play['error']}", ""]
    else:
        lines += ["| Track | Release | Version codes | Status | Review state |", "|---|---|---|---|---|"]
        for t in play["tracks"]:
            if not t["releases"]:
                lines.append(f"| {t['track']} | — | — | no releases | — |")
            for r in t["releases"]:
                codes = ", ".join(map(str, r["version_codes"]))
                lines.append(f"| {t['track']} | {r['name']} | {codes} | {r['status']} | {r['state'] or '—'} |")
        languages = ", ".join(lst["language"] for lst in play["listings"]) or "none"
        lines += ["", f"Default language: `{play['details'].get('defaultLanguage')}`; listings: {languages}", ""]
        lines += [f"> {w}" for w in play["warnings"]]
    lines += ["### Findings", ""]
    lines += [f"- **{f['severity']}** {f['message']}" for f in report["findings"]] or ["- none"]
    lines += [
        "",
        "### Upcoming policy deadlines",
        "",
        "| Deadline | Days | Policy | Declared permissions mentioned |",
        "|---|---|---|---|",
    ]
    for d in report["policy_deadlines"]["entries"]:
        if d["days_left"] >= 0:
            lines.append(
                f"| {d['deadline']} | {d['days_left']} | {d['policy']} | {', '.join(d['mentions_declared_permission']) or '—'} |"
            )
    lines += ["", "### Not visible via API (check Play Console)", ""] + [
        f"- {c}" for c in report["console_only_checks"]
    ]
    return "\n".join(lines) + "\n"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--package", default=PACKAGE)
    ap.add_argument("--key-file", default="service-account-play.json")
    ap.add_argument("--markdown", action="store_true", help="emit a Markdown summary instead of JSON")
    ap.add_argument("--summary", type=Path, help="also append the Markdown summary to this file")
    args = ap.parse_args()

    today = dt.date.today()
    play, error = None, None
    try:
        play = collect_play_state(AuthorizedSession(load_credentials(args.key_file)), args.package)
    except (RuntimeError, OSError, ValueError) as exc:
        error = str(exc)

    page = requests.get(DEADLINES_URL, timeout=30)
    page.raise_for_status()
    deadlines = parse_deadlines(page.text, today, declared_permissions(MANIFEST))

    report = {
        "generated_at": dt.datetime.now(dt.UTC).isoformat(timespec="seconds"),
        "package": args.package,
        "play_api": {
            "error": error,
            **(play or {"details": {}, "listings": [], "tracks": [], "testers": {}, "warnings": []}),
        },
        "policy_deadlines": {"source": DEADLINES_URL, "entries": deadlines},
        "findings": findings(play, deadlines),
        "console_only_checks": CONSOLE_ONLY_CHECKS,
    }
    print(to_markdown(report) if args.markdown else json.dumps(report, indent=2))
    if args.summary:
        with args.summary.open("a") as fh:
            fh.write(to_markdown(report))
    return 2 if error else 0


if __name__ == "__main__":
    sys.exit(main())
