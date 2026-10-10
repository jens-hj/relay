#!/usr/bin/env python3
"""Copy Git-listed Relay sources for an uncommitted local flake deployment."""

import argparse
from pathlib import Path
import shutil
import subprocess
import tempfile


def snapshot(root: Path, destination: Path) -> None:
    files = subprocess.check_output(["git", "ls-files", "--cached", "-z"], cwd=root)
    destination.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="relay-source-", dir=destination.parent) as temporary:
        staged = Path(temporary) / "source"
        staged.mkdir()
        for name in files.decode().split("\0"):
            if not name:
                continue
            relative = Path(name)
            if relative.parts[0] not in {"crates", "nix", "profiles"} and name not in {
                "Cargo.toml", "Cargo.lock", "flake.nix", "flake.lock", "LICENSE",
            }:
                continue
            if {"target", ".git", "data", "__pycache__"}.intersection(relative.parts):
                continue
            source = root / relative
            if not source.is_file():
                continue
            if source.is_symlink():
                raise SystemExit(f"Source snapshot refuses symlink: {relative}")
            target = staged / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, target)
        if destination.exists():
            shutil.rmtree(destination)
        staged.rename(destination)
    print(f"Prepared filtered Relay source: {destination}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    snapshot(args.root.resolve(), args.destination.resolve())
