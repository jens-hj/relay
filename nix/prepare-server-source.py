#!/usr/bin/env python3
"""Stage only the server workspace, preserving published dependency versions."""

import argparse
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import tomllib


def stage(root: Path, destination: Path, lock: Path) -> None:
    manifest = (root / "Cargo.toml").read_text()
    metadata = tomllib.loads(manifest)
    members = ["crates/relay-core", "crates/relay-server"]
    if not set(members).issubset(metadata["workspace"]["members"]):
        raise SystemExit("Relay workspace must contain core and server")
    manifest = re.sub(
        r"^members\s*=.*$", f"members = {members!r}", manifest, count=1, flags=re.M
    )
    manifest = re.sub(r"^mosaic\s*=.*\n", "", manifest, flags=re.M)
    destination.mkdir(parents=True, exist_ok=True)
    (destination / "Cargo.toml").write_text(manifest)
    shutil.copyfile(lock, destination / "Cargo.lock")
    shutil.copytree(root / "profiles", destination / "profiles", dirs_exist_ok=True)
    for member in members:
        shutil.copytree(root / member, destination / member, dirs_exist_ok=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("destination", type=Path)
    parser.add_argument("--lock", type=Path)
    parser.add_argument("--update-lock", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    if not args.update_lock:
        stage(root, args.destination, args.lock or root / "nix/server-Cargo.lock")
        return
    with tempfile.TemporaryDirectory(prefix="relay-server-lock-") as directory:
        destination = Path(directory)
        stage(root, destination, root / "Cargo.lock")
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1", "--no-deps"],
            cwd=destination,
            stdout=subprocess.DEVNULL,
            check=True,
        )
        # Force the resolver to prune unused desktop packages without selecting
        # versions outside the main lock. Metadata --no-deps alone need not prune.
        subprocess.run(
            ["cargo", "metadata", "--format-version", "1"],
            cwd=destination,
            stdout=subprocess.DEVNULL,
            check=True,
        )
        original = tomllib.loads((root / "Cargo.lock").read_text())["package"]
        generated = tomllib.loads((destination / "Cargo.lock").read_text())["package"]
        identities = {(p["name"], p["version"], p.get("source")) for p in original}
        for package in generated:
            identity = (package["name"], package["version"], package.get("source"))
            if identity not in identities:
                raise SystemExit(f"Server lock introduced an unpinned package: {identity}")
            if package.get("source", "").startswith("git+"):
                raise SystemExit("Server-only lock unexpectedly includes a Git dependency")
        target = root / "nix/server-Cargo.lock"
        shutil.copyfile(destination / "Cargo.lock", target)
        print(f"Updated {target}: {len(generated)} packages")


if __name__ == "__main__":
    main()
