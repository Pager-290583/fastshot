#!/usr/bin/env python3
"""Generate flatpak-builder sources for all crates in Cargo.lock (offline build).

Minimal, dependency-free replacement for flatpak-cargo-generator.py
(only crates.io packages are supported, which is all Fastshot uses).

usage: cargo-sources.py ../Cargo.lock > cargo-sources.json
"""
import json
import sys
import tomllib

CRATES = "https://static.crates.io/crates"
VENDOR = "cargo/vendor"

lock = tomllib.load(open(sys.argv[1], "rb"))
sources = []
for pkg in lock["package"]:
    src = pkg.get("source", "")
    if not src:
        continue  # the workspace itself
    if not src.startswith("registry+"):
        sys.exit(f"unsupported source for {pkg['name']}: {src}")
    name, version, checksum = pkg["name"], pkg["version"], pkg["checksum"]
    dest = f"{VENDOR}/{name}-{version}"
    sources.append({
        "type": "archive",
        "archive-type": "tar-gzip",
        "url": f"{CRATES}/{name}/{name}-{version}.crate",
        "sha256": checksum,
        "dest": dest,
    })
    sources.append({
        "type": "inline",
        "contents": json.dumps({"package": checksum, "files": {}}),
        "dest": dest,
        "dest-filename": ".cargo-checksum.json",
    })
sources.append({
    "type": "inline",
    "contents": (
        '[source.vendored-sources]\ndirectory = "cargo/vendor"\n\n'
        '[source.crates-io]\nreplace-with = "vendored-sources"\n'
    ),
    "dest": "cargo",
    "dest-filename": "config.toml",
})
json.dump(sources, sys.stdout, indent=2)
print()
