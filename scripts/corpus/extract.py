#!/usr/bin/env python3
"""Extract every Metis document under a root into JSONL, chunked on heading
boundaries exactly as KAIROS-T-0190 specifies, so the same fixture serves both
the structural measurement and the cosine measurement."""
import json, os, re, sys, hashlib

ROOT = sys.argv[1]
OUT = sys.argv[2]
CEILING = 2000          # split a section again above this
WINDOW, OVERLAP = 1200, 200

HEADING = re.compile(r'^(#{1,6})\s+(.*?)\s*#*$')

def frontmatter(text):
    if not text.startswith('---'):
        return {}, text
    end = text.find('\n---', 3)
    if end < 0:
        return {}, text
    fm = {}
    for line in text[3:end].splitlines():
        m = re.match(r'^([a-z_]+):\s*(.*)$', line)
        if m:
            fm[m.group(1)] = m.group(2).strip().strip('"')
    return fm, text[end+4:]

def window(s, start_off):
    out, i = [], 0
    while i < len(s):
        out.append((s[i:i+WINDOW], start_off + i))
        if i + WINDOW >= len(s):
            break
        i += WINDOW - OVERLAP
    return out

def chunk(body):
    """Heading boundaries as anchors. Returns (heading_or_None, text, offset)."""
    lines = body.splitlines(keepends=True)
    secs, cur, head, off, curoff = [], [], None, 0, 0
    for ln in lines:
        m = HEADING.match(ln.rstrip('\n'))
        if m:
            if ''.join(cur).strip():
                secs.append((head, ''.join(cur), curoff))
            head, cur, curoff = m.group(2), [], off + len(ln)
        else:
            cur.append(ln)
        off += len(ln)
    if ''.join(cur).strip():
        secs.append((head, ''.join(cur), curoff))
    out, split = [], 0
    for h, t, o in secs:
        if len(t) <= CEILING:
            out.append((h, t, o))
        else:
            split += 1
            out.extend((h, w, wo) for w, wo in window(t, o))
    return out, len(secs), split

docs = []
for dirpath, dirnames, filenames in os.walk(ROOT):
    if '.metis' not in dirpath.split(os.sep):
        continue
    for fn in filenames:
        if not fn.endswith('.md'):
            continue
        p = os.path.join(dirpath, fn)
        try:
            text = open(p, encoding='utf-8', errors='replace').read()
        except OSError:
            continue
        fm, body = frontmatter(text)
        lvl = fm.get('level') or ('index' if fn == 'MEMORY.md' else 'unknown')
        if lvl == 'unknown' and fn in ('code-index.md', 'vision.md'):
            lvl = 'vision' if fn == 'vision.md' else 'index'
        if lvl in ('unknown', 'index'):
            continue
        rel = os.path.relpath(p, ROOT)
        parts = rel.split(os.sep)
        # only each repository's OWN top-level .metis: git worktrees under
        # .claude/worktrees/ and vendored prior-art carry full copies, which
        # would otherwise flood the measurement with cosine-1.0 duplicates.
        if len(parts) < 3 or parts[1] != '.metis':
            continue
        project = parts[0]
        archived = 'archived' in rel.split(os.sep) or fm.get('archived') == 'true'
        ch, n_sections, n_split = chunk(body)
        docs.append({
            'path': rel, 'project': project, 'level': lvl,
            'short_code': fm.get('short_code', ''), 'title': fm.get('title', ''),
            'parent': fm.get('parent', ''), 'initiative_id': fm.get('initiative_id', ''),
            'n_sections': n_sections, 'n_split': n_split,
            'archived': archived,
            'chars': len(body), 'n_headings': sum(1 for h, _, _ in ch if h),
            'headings': [h for h, _, _ in ch],
            'chunks': [{'h': h, 'off': o, 'chars': len(t), 'text': t} for h, t, o in ch],
        })

# belt and braces: identical bodies anywhere in the corpus collapse to one
seen, deduped = set(), []
for d in docs:
    h = hashlib.sha256(''.join(c['text'] for c in d['chunks']).encode()).hexdigest()
    if h in seen:
        continue
    seen.add(h)
    deduped.append(d)
print(f'dropped {len(docs)-len(deduped)} duplicate-content documents')
docs = deduped

with open(OUT, 'w') as f:
    for d in docs:
        f.write(json.dumps(d) + '\n')
print(f"{len(docs)} documents, {sum(len(d['chunks']) for d in docs)} chunks -> {OUT}")
