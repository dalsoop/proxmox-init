#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from fnmatch import fnmatch
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
DOMAINS_NCL = ROOT / "ncl" / "domains.ncl"
POLICY_NCL = ROOT / "ncl" / "policies" / "architecture.ncl"
PXI_REF_RE = re.compile(r"\bpxi-([a-z][a-z0-9-]*)\b")


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


def load_json(ncl_path: Path) -> dict:
    out = run(["nickel", "export", "--format", "json", str(ncl_path)])
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
    out = run(["git", "diff", "--cached", "--name-only", "--diff-filter=ACMR"], cwd=ROOT)
    return [line.strip() for line in out.splitlines() if line.strip()]


def file_lines(path: str) -> list[tuple[int, str]]:
    try:
        text = (ROOT / path).read_text(encoding="utf-8", errors="ignore")
    except FileNotFoundError:
        return []
    return list(enumerate(text.splitlines(), start=1))


def domain_name_for(path: str) -> str | None:
    parts = Path(path).parts
    try:
        idx = parts.index("domains")
    except ValueError:
        return None
    if idx + 1 < len(parts):
        return parts[idx + 1]
    return None


def matches_any(path: str, globs: list[str]) -> bool:
    return any(fnmatch(path, pattern) for pattern in globs)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--staged", action="store_true")
    parser.add_argument("--paths", nargs="*")
    args = parser.parse_args()

    if args.paths:
        paths = [repo_rel(p) for p in args.paths]
    elif args.staged:
        paths = staged_files()
    else:
        parser.error("use --staged or --paths")

    if not paths:
        print("architecture-lint: no files to check")
        return 0

    policy = load_json(POLICY_NCL)
    registry = load_json(DOMAINS_NCL)
    domains = registry["domains"]
    known_domains = set(domains.keys())

    errors: list[str] = []
    for path in paths:
        if not matches_any(path, policy.get("scan_globs", [])):
            continue
        domain = domain_name_for(path)
        if not domain or domain not in known_domains:
            continue
        requires = set(domains[domain].get("requires", []))
        ignore = set(policy.get("ignore_refs", {}).get(domain, []))
        for lineno, line in file_lines(path):
            for match in PXI_REF_RE.finditer(line):
                ref = match.group(1)
                if ref == domain or ref in ignore:
                    continue
                if ref in known_domains and ref not in requires:
                    errors.append(
                        f"{path}:{lineno}: references pxi-{ref} but {domain}.domain.ncl requires does not include '{ref}'"
                    )

    if errors:
        print("architecture-lint: failed", file=sys.stderr)
        for err in errors:
            print(f"  - {err}", file=sys.stderr)
        return 1

    print("architecture-lint: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
