#!/usr/bin/env python3
"""Generate latest.json for Tauri updater from release assets and .sig files.

Usage:
    VERSION=v0.1.0 REPO=shuaijinchao/nyro python3 scripts/gen_latest_json.py \
        --assets-dir release-assets --output latest.json

Environment variables:
    VERSION   Git tag name, e.g. v0.1.0
    REPO      GitHub owner/repo, e.g. shuaijinchao/nyro
"""
import argparse
import json
import os
import sys
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import quote


PLATFORM_RULES = [
    # (file suffix, arch keywords, platform key, score)
    (".app.tar.gz", ("x86_64", "_x64", "-x64"), "darwin-x86_64", 100),
    (".app.tar.gz", ("aarch64", "arm64"),        "darwin-aarch64", 100),
    (".app.tar.gz", (),                           "darwin-aarch64",  99),
    ("-setup.exe",  (),                           "windows-x86_64", 100),
    (".msi",        (),                           "windows-x86_64",  50),
    (".appimage",   ("aarch64", "arm64"),         "linux-aarch64",  100),
    (".appimage",   (),                           "linux-x86_64",   100),
    (".deb",        ("arm64", "aarch64"),         "linux-aarch64",   30),
    (".deb",        (),                           "linux-x86_64",    30),
]


def classify(name: str):
    lower = name.lower()
    for suffix, keywords, platform, score in PLATFORM_RULES:
        if not lower.endswith(suffix):
            continue
        if not keywords or any(k in lower for k in keywords):
            return platform, score
    return None, -1


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--assets-dir", default="release-assets")
    parser.add_argument("--output", default="latest.json")
    args = parser.parse_args()

    version_tag = os.environ.get("VERSION", "").strip()
    repo = os.environ.get("REPO", "").strip()
    if not version_tag or not repo:
        sys.exit("ERROR: VERSION and REPO environment variables are required.")

    version = version_tag.lstrip("v")
    release_dir = Path(args.assets_dir)
    sig_files = sorted(release_dir.glob("*.sig"))
    if not sig_files:
        sys.exit(
            f"ERROR: No .sig files found in '{release_dir}'. "
            "Ensure TAURI_SIGNING_PRIVATE_KEY is configured."
        )

    base_url = f"https://github.com/{repo}/releases/download/{version_tag}"
    preferred: dict = {}

    for sig in sig_files:
        target_name = sig.name[:-4]
        platform, score = classify(target_name)
        if platform is None:
            continue
        target_file = release_dir / target_name
        if not target_file.exists():
            continue
        current = preferred.get(platform)
        if current is None or score > current["score"]:
            preferred[platform] = {
                "file": target_name,
                "signature": sig.read_text(encoding="utf-8").strip(),
                "score": score,
            }

    platforms = {
        p: {"url": f"{base_url}/{quote(e['file'])}", "signature": e["signature"]}
        for p, e in preferred.items()
    }

    if not platforms:
        sys.exit("ERROR: No updater-compatible assets found to build latest.json.")

    latest = {
        "version": version,
        "notes": "See release notes for details.",
        "pub_date": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": platforms,
    }
    Path(args.output).write_text(
        json.dumps(latest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(f"Generated {args.output} — platforms: {', '.join(platforms)}")


if __name__ == "__main__":
    main()
