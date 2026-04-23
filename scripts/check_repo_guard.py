#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from fnmatch import fnmatch
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
POLICY = ROOT / "ncl" / "policies" / "repo_guard.ncl"


def run(cmd: list[str], cwd: Path | None = None) -> str:
    proc = subprocess.run(
        cmd,
        cwd=str(cwd or ROOT),
        text=True,
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(f"{' '.join(cmd)} failed: {proc.stderr.strip()}")
    return proc.stdout


def load_policy() -> dict:
    out = run(["nickel", "export", "--format", "json", str(POLICY)], cwd=ROOT)
    return json.loads(out)


def repo_rel(path: str) -> str:
    p = Path(path)
    if p.is_absolute():
        try:
            return p.relative_to(ROOT).as_posix()
        except ValueError:
            return p.as_posix()
    return p.as_posix()


def staged_files() -> list[str]:
    out = run(
        ["git", "diff", "--cached", "--name-only", "--diff-filter=ACMR"],
        cwd=ROOT,
    )
    return [line.strip() for line in out.splitlines() if line.strip()]


def staged_added_lines(path: str) -> list[str]:
    out = run(["git", "diff", "--cached", "--unified=0", "--", path], cwd=ROOT)
    added: list[str] = []
    for line in out.splitlines():
        if line.startswith("+++") or line.startswith("@@"):
            continue
        if line.startswith("+"):
            added.append(line[1:])
    return added


def file_lines(path: str) -> list[str]:
    try:
        return (ROOT / path).read_text(encoding="utf-8", errors="ignore").splitlines()
    except FileNotFoundError:
        return []


def matches_any(path: str, globs: list[str]) -> bool:
    return any(fnmatch(path, pattern) for pattern in globs)


def check_blocked_paths(paths: list[str], policy: dict) -> list[str]:
    errors = []
    blocked = policy.get("blocked_paths", [])
    for path in paths:
        if matches_any(path, blocked):
            errors.append(f"blocked path staged: {path}")
    return errors


def check_hardcoded(paths: list[str], policy: dict, use_staged: bool) -> list[str]:
    errors = []
    for rule in policy.get("hardcoded_rules", []):
        pattern = re.compile(rule["regex"])
        allowed = rule.get("allowed_globs", [])
        for path in paths:
            if matches_any(path, allowed):
                continue
            lines = staged_added_lines(path) if use_staged else file_lines(path)
            for idx, line in enumerate(lines, start=1):
                if pattern.search(line):
                    errors.append(
                        f"{rule['name']}: forbidden hardcoded value in {path}:{idx}: {line.strip()}"
                    )
    return errors


def check_companion_rules(paths: list[str], policy: dict) -> list[str]:
    errors = []
    for rule in policy.get("companion_rules", []):
        triggers = rule.get("trigger_globs", [])
        reqs = rule.get("requires_any_globs", [])
        if any(matches_any(path, triggers) for path in paths):
            if not any(matches_any(path, reqs) for path in paths):
                errors.append(f"{rule['name']}: {rule['message']}")
    return errors


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--staged", action="store_true")
    parser.add_argument("--paths", nargs="*")
    args = parser.parse_args()

    if args.paths:
        paths = [repo_rel(p) for p in args.paths]
        use_staged = False
    elif args.staged:
        paths = staged_files()
        use_staged = True
    else:
        parser.error("use --staged or --paths")

    if not paths:
        print("repo-guard: no files to check")
        return 0

    policy = load_policy()
    errors: list[str] = []
    errors += check_blocked_paths(paths, policy)
    errors += check_hardcoded(paths, policy, use_staged)
    errors += check_companion_rules(paths, policy)

    if errors:
        print("repo-guard: failed", file=sys.stderr)
        for err in errors:
            print(f"  - {err}", file=sys.stderr)
        return 1

    print("repo-guard: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
