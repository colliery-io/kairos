# Mermaid (vendored)

KAIROS-T-0324: the GUI draws ```` ```mermaid ```` fences with Mermaid. The
file is vendored so that a deployment with no internet access works, and it
is loaded only by a page that has a mermaid fence (`index.html`).

| | |
|---|---|
| Package | `mermaid` 12.1.0 (MIT, see `LICENSE`) |
| File | `dist/mermaid.min.js` of the npm tarball, renamed `mermaid-12.1.0.min.js` |
| Source | https://registry.npmjs.org/mermaid/-/mermaid-12.1.0.tgz |
| Tarball integrity (npm) | `sha512-wlVCp+8eTupfCeeFvoZNNiTuHrvag0P2jz/ILgb/f/6jkVokUefOcujefi8qUe/j2asHiSePncVsz/xzzA80LQ==` |
| SHA-256 of the file | `6484afc32872a3aa16cac9a76ba1816a1ed4cc870a6593cc2e17757750f518b2` |

The version is in the file name because the server caches each asset as
immutable. To upgrade: take `dist/mermaid.min.js` from the new tarball, check
the tarball against its npm integrity, rename the file with the new version,
change the two references in `crates/kairos-web/index.html`, and update this
table.
