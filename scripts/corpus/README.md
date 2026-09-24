# Corpus measurement (KAIROS-I-0017)

The retrieval design in [KAIROS-A-0021](../../.metis/adrs/KAIROS-A-0021.md) rests
on measurements of real Metis documents rather than on assumptions about them.
This directory is how those measurements were taken, so they can be retaken when
the model, the chunker or the primary-vector composition changes.

The sample is nineteen Metis corpora — 4,927 documents — because Kairos's own
238 documents are too few to tell a real distribution from a sampling artefact,
and because a design that only works on one project's writing habits is not a
design. The full numbers live in
[KAIROS-T-0190](../../.metis/initiatives/KAIROS-I-0017/tasks/KAIROS-T-0190.md).

## Extract and measure structure

```sh
python3 scripts/corpus/extract.py <root-containing-repos> corpus.jsonl
python3 scripts/corpus/stats.py corpus.jsonl
```

`extract.py` chunks on heading boundaries exactly as A-0021 rule 3 specifies, so
the fixture it writes is the chunker's own behaviour rather than an approximation
of it. It takes only each repository's own top-level `.metis`, and deduplicates by
content hash: git worktrees under `.claude/worktrees/` and vendored `prior-art`
trees carry full copies, and including them added 3,774 phantom documents that
flooded the first measurement with cosine-1.000 self-pairs.

## Embed and measure similarity

`embed.rs` and `analyze.rs` were a throwaway crate — `fastembed = "7"` and
`serde_json = "1"`, `embed.rs` as `src/main.rs` and `analyze.rs` as
`src/bin/analyze.rs`. They are kept here as source rather than as a workspace
member because the real implementation lands in `kairos-core` under
[KAIROS-T-0189](../../.metis/initiatives/KAIROS-I-0017/tasks/KAIROS-T-0189.md),
and a second copy of it in the workspace would rot.

```sh
cargo run --release -- corpus.jsonl <outdir>          # writes primary.f32 + .idx
cargo run --release --bin analyze -- <outdir>
```

`analyze.rs` computes **full** pairwise cosine, not a sample, and classifies every
pair by what the graph says about it — no relation, shared parent, direct edge.
That classification is the whole point: the headline result is that related and
unrelated pairs overlap across their entire useful range, which is only visible if
you separate them.
