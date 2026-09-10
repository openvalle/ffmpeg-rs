#!/usr/bin/env python3
"""Build a pinned, minimal FFmpeg shared-library set for native CI tests only."""
import argparse
import hashlib
import os
from pathlib import Path
import subprocess
import tarfile
import urllib.request

HASHES = {
    7: "4426a94dd2c814945456600c8adfc402bee65ec14a70e8c531ec9a2cd651da7b",
    8: "b2751fccb6cc4c77708113cd78b561059b6fa904b24162fa0be2d60273d27b8e",
    9: "7f607a00dd0d28a729d5a4811205812eef01cf6ef6155025febb6f36a9062d52",
}

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("major", type=int, choices=HASHES)
parser.add_argument("--directory", type=Path, required=True)
args = parser.parse_args()
root = args.directory.resolve()
root.mkdir(parents=True, exist_ok=True)
archive = root / f"ffmpeg-{args.major}.0.tar.xz"
if not archive.exists():
    urllib.request.urlretrieve(f"https://ffmpeg.org/releases/{archive.name}", archive)
if hashlib.sha256(archive.read_bytes()).hexdigest() != HASHES[args.major]:
    raise SystemExit("FFmpeg source archive SHA-256 mismatch")
source = root / f"ffmpeg-{args.major}.0"
if not source.exists():
    with tarfile.open(archive) as tar:
        for member in tar.getmembers():
            target = (root / member.name).resolve()
            if root not in target.parents or member.issym() or member.islnk() or member.isdev():
                raise SystemExit(f"unexpected archive member: {member.name}")
        tar.extractall(root)
prefix = root / "runtime"
subprocess.run([
    str(source / "configure"), f"--prefix={prefix}", "--disable-everything",
    "--disable-autodetect", "--disable-programs", "--disable-doc", "--disable-network",
    "--disable-x86asm", "--enable-shared", "--disable-static", "--enable-encoder=ffv1",
    "--enable-decoder=ffv1", "--enable-muxer=matroska", "--enable-demuxer=matroska",
    "--enable-protocol=file", "--enable-filter=overlay",
], cwd=source, check=True)
subprocess.run(["make", f"-j{min(os.cpu_count() or 2, 8)}"], cwd=source, check=True)
subprocess.run(["make", "install-libs"], cwd=source, check=True)
print(prefix / "lib")
