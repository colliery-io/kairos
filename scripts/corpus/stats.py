import json, statistics as st, collections, sys
docs = [json.loads(l) for l in open(sys.argv[1])]

def pct(v, p):
    v = sorted(v); return v[min(len(v)-1, int(len(v)*p/100))]

print(f"{len(docs)} documents across {len(set(d['project'] for d in docs))} projects, "
      f"{sum(len(d['chunks']) for d in docs)} chunks, "
      f"{sum(d['chars'] for d in docs)/1e6:.1f} MB of body text\n")

print("per project")
print(f"{'project':<16}{'docs':>6}{'chunks':>8}{'median chars':>14}{'no-heading docs':>17}")
by = collections.defaultdict(list)
for d in docs: by[d['project']].append(d)
for p, ds in sorted(by.items(), key=lambda kv: -len(kv[1])):
    nh = sum(1 for d in ds if d['n_headings'] == 0)
    print(f"{p:<16}{len(ds):>6}{sum(len(d['chunks']) for d in ds):>8}"
          f"{int(st.median(d['chars'] for d in ds)):>14}{nh:>17}")

print("\nper level (all projects)")
print(f"{'level':<16}{'n':>6}{'mean':>8}{'p50':>8}{'p90':>8}{'max':>8}{'>2000':>8}")
byl = collections.defaultdict(list)
for d in docs: byl[d['level']].append(d['chars'])
for l, v in sorted(byl.items(), key=lambda kv: -len(kv[1])):
    print(f"{l:<16}{len(v):>6}{int(st.mean(v)):>8}{pct(v,50):>8}{pct(v,90):>8}{max(v):>8}"
          f"{100*sum(1 for x in v if x>2000)//len(v):>7}%")

sec = [c['chars'] for d in docs for c in d['chunks']]
print(f"\nchunks: n={len(sec)} mean={int(st.mean(sec))} p50={pct(sec,50)} "
      f"p90={pct(sec,90)} p99={pct(sec,99)} max={max(sec)} "
      f"under2000={100*sum(1 for x in sec if x<=2000)//len(sec)}%")
nohead = sum(1 for d in docs if d['n_headings'] == 0)
print(f"documents with no headings: {nohead} ({100*nohead/len(docs):.1f}%)")
print(f"median sections per doc: {int(st.median(len(d['chunks']) for d in docs))}")

hs = collections.Counter(h for d in docs for h in d['headings'] if h)
once = sum(1 for v in hs.values() if v == 1)
print(f"\nheadings: {len(hs)} distinct, {sum(hs.values())} total, "
      f"{100*once//len(hs)}% appear exactly once")
cum, tot = 0, sum(hs.values())
for i, (h, c) in enumerate(hs.most_common(10), 1):
    cum += c
    print(f"  {i:>2}. {c:>6} ({100*cum//tot:>2}% cum)  {h[:62]}")
