#!/usr/bin/env python3
"""Run each FFmpeg ABI in its own process, on Windows, Linux or macOS."""
import argparse
import os
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--root", type=Path, help="parent of ffmpeg-7/8/9/runtime directories")
args = parser.parse_args()
env = os.environ.copy()
for major in (7, 8, 9):
    key = f"VALLE_TEST_FFMPEG_{major}_DIR"
    if args.root:
        env[key] = str((args.root / f"ffmpeg-{major}" / "runtime").resolve())
    if key not in env:
        parser.error(f"set {key} or pass --root")

subprocess.run(["cargo", "test", "-p", "valle-ffmpeg", "--locked", "--test", "runtime",
                "native_runtime_matrix", "--", "--ignored", "--exact"], env=env, check=True)
for major in (7, 8, 9):
    env["VALLE_FFMPEG_DIR"] = env[f"VALLE_TEST_FFMPEG_{major}_DIR"]
    # Includes inherited wrapper unit tests and the common decoder's PTS/property regression.
    for target in (["--lib"], ["--test", "upstream"]):
        subprocess.run(["cargo", "test", "-p", "valle-ffmpeg", "--locked", *target,
                        f"v{major}::", "--", "--ignored", "--test-threads=1"], env=env, check=True)
