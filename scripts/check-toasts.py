#!/usr/bin/env python3
"""Toast wording: every call site speaks the same way (see toast.rs).

Words are Capitalized, one to five of them, with all caps only for what is
all caps (an acronym on the list below), no "!" and no "·" — that belongs in
the detail. Runs on the source; no build needed.
"""
from pathlib import Path
import re
import sys

src = Path(__file__).resolve().parent.parent / 'spikes/composite/src'
ACRONYMS = {'URL', 'HTTP', 'HTTPS', 'SSH', 'PDF', 'LSP', 'CLI', 'API', 'CSS', 'HTML', 'JSON', 'PIP', 'GPU', 'OS', 'ID'}
LOWER = {'nus'}  # the name keeps its own case
WORDS = re.compile(r'\.(toast|notice)\(\s*[\w:]+\s*,\s*(?:format!\()?"([^"]*)"|\.(toast_problem|notice_problem)\(\s*(?:format!\()?"([^"]*)"', re.S)
OLD = re.compile(r'\.toast_with\(|\.notice\(\s*&|\.notice\(\s*"')

def problems(words):
    found = []
    if '!' in words:
        found.append('no "!"')
    if '·' in words:
        found.append('"·" goes in the detail')
    parts = [w for w in words.split() if not re.fullmatch(r'\{[^}]*\}', w)]
    if not 1 <= len(words.split()) <= 5:
        found.append('one to five words')
    for w in parts:
        letters = re.sub(r'[^A-Za-z]', '', w)
        if not letters or w in LOWER:
            continue
        if len(letters) > 1 and letters.isupper() and letters not in ACRONYMS:
            found.append(f'{w!r} is all caps (not on the acronym list)')
        elif not w.lstrip('@(\'"')[:1].isupper() and not w[:1].isdigit() and not w.startswith('{'):
            found.append(f'{w!r} is not Capitalized')
    return found

bad = 0
seen = 0
for path in sorted(src.glob('*.rs')):
    text = path.read_text()
    for m in WORDS.finditer(text):
        words = m.group(2) if m.group(2) is not None else m.group(4)
        seen += 1
        line = text.count('\n', 0, m.start()) + 1
        for p in problems(words):
            bad += 1
            print(f'{path.name}:{line}: "{words}": {p}')
    for m in OLD.finditer(text):
        bad += 1
        print(f'{path.name}:{text.count(chr(10), 0, m.start()) + 1}: old toast API')
print(f'{seen} toasts checked, {bad} problems')
sys.exit(1 if bad or seen == 0 else 0)
