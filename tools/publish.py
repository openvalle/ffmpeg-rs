#!/usr/bin/env python3
"""Validate a release plan; upload only with --publish. Resume identical published packages."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import time
import urllib.error
import urllib.request

PACKAGES = ('valle-ffmpeg-sys', 'valle-ffmpeg')
ROOT = Path(__file__).resolve().parent.parent


def index_version(name, version):
    # Public sparse index only. The Cargo token stays with cargo publish.
    request = urllib.request.Request(f'https://index.crates.io/{name[:2]}/{name[2:4]}/{name}',
                                     headers={'User-Agent': 'openvalle-ffmpeg-rs-release',
                                              'Cache-Control': 'no-cache'})
    for attempt in range(3):
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                records = response.read().decode().splitlines()
            return next((entry for line in records if (entry := json.loads(line))['vers'] == version), None)
        except urllib.error.HTTPError as error:
            if error.code == 404:
                return None
            if error.code not in (408, 429, 500, 502, 503, 504) or attempt == 2:
                raise
        except (urllib.error.URLError, TimeoutError):
            if attempt == 2:
                raise
        time.sleep(2 ** attempt)
    raise AssertionError('unreachable')


def identical(entry, name, version, checksum):
    if entry is None:
        return False
    if entry.get('name') != name or entry.get('vers') != version:
        raise RuntimeError(f'{name} {version}: unexpected registry identity')
    if entry.get('yanked'):
        raise RuntimeError(f'{name} {version} is yanked; choose a new version')
    if entry['cksum'] != checksum:
        raise RuntimeError(f'{name} {version} already exists with different package bytes; '
                           'resume from the original revision/toolchain or choose a new version')
    return True


def wait_for_index(name, version, checksum, timeout=180):
    deadline = time.monotonic() + timeout
    while True:
        if identical(index_version(name, version), name, version, checksum):
            return
        if time.monotonic() >= deadline:
            raise RuntimeError(f'{name} {version} is not yet visible in the index; '
                               'rerun this workflow at the same revision to resume safely')
        time.sleep(5)


def publish_package(name, version, checksum, upload):
    if identical(index_version(name, version), name, version, checksum):
        print(f'{name} {version}: identical package already published; continue', flush=True)
        return
    if not upload:
        print(f'{name} {version}: ready to publish (no upload requested)', flush=True)
        return
    # Cargo already polls the index. Do not retry uploads on ambiguous failures.
    result = subprocess.run(['cargo', 'publish', '-p', name, '--locked', '--registry', 'crates-io'], cwd=ROOT)
    if result.returncode:
        print(f'{name}: cargo exited {result.returncode}; checking whether upload reached the index', flush=True)
    wait_for_index(name, version, checksum)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--publish', action='store_true')
    args = parser.parse_args()
    if subprocess.check_output(['git', 'status', '--porcelain'], cwd=ROOT):
        raise SystemExit('release requires a clean working tree')
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--no-deps', '--format-version', '1', '--locked'], cwd=ROOT))
    packages = {p['name']: p for p in metadata['packages'] if p['name'] in PACKAGES}
    if len(packages) != 2 or len({p['version'] for p in packages.values()}) != 1:
        raise SystemExit('both release packages must share one version')
    # Recreate archives at this revision; never trust stale target/package files.
    subprocess.run(['cargo', 'package', '--workspace', '--locked'], cwd=ROOT, check=True)
    archives = Path(metadata['target_directory']) / 'package'
    plan = [(name, packages[name]['version'], hashlib.sha256(
        (archives / f"{name}-{packages[name]['version']}.crate").read_bytes()).hexdigest()) for name in PACKAGES]
    # Preflight both packages before the first irreversible upload.
    for name, version, checksum in plan:
        identical(index_version(name, version), name, version, checksum)
    for name, version, checksum in plan:
        publish_package(name, version, checksum, args.publish)


if __name__ == '__main__':
    main()
