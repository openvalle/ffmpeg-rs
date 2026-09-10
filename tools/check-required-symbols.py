#!/usr/bin/env python3
"""Check native function references against required-symbols.txt.

The scope is wrapper/src and sys/src/avutil, across every source cfg branch.
Tokens (including use aliases and macro bodies) are intersected with bindgen's
actual function inventories for ABI 7/8/9 on this target with all Cargo features.
This conservative lexical check is not a proof about computed symbol names or
APIs excluded by the target SDK. CI runs it on Linux, macOS and Windows.
"""
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parent.parent


def identifiers(source):
    """Ignore comments/literals, including nested block comments and raw strings."""
    names = set()
    i = 0
    while i < len(source):
        if source.startswith('//', i):
            end = source.find('\n', i)
            i = len(source) if end < 0 else end + 1
        elif source.startswith('/*', i):
            depth = 1
            i += 2
            while depth and i < len(source):
                if source.startswith('/*', i):
                    depth += 1
                    i += 2
                elif source.startswith('*/', i):
                    depth -= 1
                    i += 2
                else:
                    i += 1
            if depth:
                raise ValueError('unterminated Rust block comment')
        elif raw := re.match(r'(?:br|cr|r)(#*)"', source[i:]):
            end = source.find('"' + raw[1], i + raw.end())
            if end < 0:
                raise ValueError('unterminated Rust raw string')
            i = end + 1 + len(raw[1])
        elif source[i] == '"':
            i += 1
            while i < len(source) and source[i] != '"':
                i += 2 if source[i] == '\\' else 1
            i += 1
        elif char := re.match(r"'(?:\\(?:u\{[0-9a-fA-F_]+\}|x[0-9a-fA-F]{2}|.)|[^'\\\n])'", source[i:]):
            i += char.end()
        elif name := re.match(r'[A-Za-z_][A-Za-z_0-9]*', source[i:]):
            names.add(name[0])
            i += name.end()
        else:
            i += 1
    return names


def audit(required, native, references):
    missing = references.intersection(native).difference(required)
    if missing:
        raise ValueError('native references missing from required-symbols.txt: ' + ', '.join(sorted(missing)))
    return len(references.intersection(native))


def main():
    # JSON provides the current build output directory; never scan stale target directories.
    result = subprocess.run(['cargo', 'check', '--workspace', '--all-features', '--locked',
                             '--message-format=json'], cwd=ROOT, text=True, stdout=subprocess.PIPE, check=True)
    output = None
    for line in result.stdout.splitlines():
        item = json.loads(line)
        if item.get('reason') == 'build-script-executed' and 'valle-ffmpeg-sys' in item['package_id']:
            output = Path(item['out_dir'])
    if output is None:
        raise SystemExit('cargo did not report the sys build output')
    native = set()
    for major in (7, 8, 9):
        native.update((output / f'abi{major}' / 'symbols.txt').read_text().splitlines())
    entries = [line.strip() for line in (ROOT / 'crates/ffmpeg-sys/required-symbols.txt').read_text().splitlines()
               if line.strip() and not line.lstrip().startswith('#')]
    if entries != sorted(set(entries)):
        raise SystemExit('required-symbols.txt must contain sorted, unique names')
    references = set()
    for source in ('crates/ffmpeg/src', 'crates/ffmpeg-sys/src/avutil'):
        for path in (ROOT / source).rglob('*.rs'):
            references.update(identifiers(path.read_text()))
    count = audit(set(entries), native, references)
    print(f'{count} referenced native functions, {len(entries)} required entries, 0 missing '
          '(all ABI source branches; target-specific generated functions)')


if __name__ == '__main__':
    main()
