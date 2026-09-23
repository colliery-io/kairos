---
id: the-amd64-only-image-blocks-every
level: task
title: "The amd64-only image blocks every ARM evaluation, including local kind clusters"
short_code: "KAIROS-T-0180"
created_at: 2026-09-23T23:21:35.994278+00:00
updated_at: 2026-09-23T23:21:35.994278+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#phase/backlog"
  - "#bug"


exit_criteria_met: false
initiative_id: NULL
---

# The amd64-only image blocks every ARM evaluation, including local kind clusters

## Objective

Publish a multi-arch image so Kairos can run on an arm64 Kubernetes node.

## Backlog Item Details

### Type
- [x] Bug - Production issue that needs fixing

### Priority
- [x] P1 - High (it blocks first-run evaluation on the most common laptop)

### Impact Assessment

- **Affected users**: anyone evaluating Kairos on an ARM machine — an Apple
  Silicon Mac, an ARM cloud instance, a Raspberry Pi cluster. For a local
  `kind` or `minikube` evaluation on a modern Mac this is **total**: the
  deployment cannot start at all.
- **Reproduction**, run on an Apple Silicon Mac (2026-09-23):
  1. `kind create cluster --name kairos-tutorial`
  2. Stand up a Postgres and a Dex in the cluster.
  3. `helm install kairos oci://ghcr.io/colliery-io/charts/kairos --version 0.1.0 -n kairos -f values.yaml --wait`
  4. `Error: INSTALLATION FAILED: context deadline exceeded`; the pods sit in
     `ImagePullBackOff`.
- **The error**, from the kubelet:

  ```
  Failed to pull image "ghcr.io/colliery-io/kairos:0.1.0": rpc error: code = NotFound
  desc = failed to pull and unpack image: no match for platform in manifest: not found
  ```

  and from Docker directly:

  ```
  no matching manifest for linux/arm64/v8 in the manifest list entries
  ```

- **Expected vs actual**: the chart's documented install command works on the
  reader's cluster. Instead the image's manifest list contains `linux/amd64`
  only (`release.yml`'s `image` job sets `platforms: linux/amd64`), so an
  arm64 node has nothing to pull.

### Why this is worse than the known limitation it was filed as

amd64-only was a **deliberate** v0.1.0 decision, recorded in KAIROS-T-0047:
QEMU-emulated arm64 builds of the Rust + wasm + trunk workspace are slow and
flaky, and the note in `release.yml` describes the native-runner fix. It was
treated as a nice-to-have.

What that assessment missed is **which** users it excludes. It is not "arm64
production clusters are unsupported for now" — it is that the most likely way
anybody tries Kairos for the first time, a local `kind` cluster on the laptop
they already own, fails at the first command with an error about manifests.
The chart is correct, the values are correct, and nothing works.

Emulation does not rescue it. `docker pull --platform linux/amd64` succeeds on
the host, but `kind load docker-image` does not make the image usable: the kind
node's own containerd is arm64 and rejects the manifest before emulation is
ever consulted. So there is no documentable workaround, which is what moves
this from a limitation to a defect.

## Acceptance Criteria

- [ ] `ghcr.io/colliery-io/kairos:<version>`'s manifest list includes
      `linux/arm64` as well as `linux/amd64`.
- [ ] `helm install` from the published chart succeeds on an arm64 `kind`
      cluster, verified rather than assumed.
- [ ] The release does not get materially slower — see below.

## Implementation Notes

**Do not add QEMU.** `release.yml`'s own comment explains why, and it is
right: a full from-source Rust + wasm build under emulation is slow and flaky.

The fix it describes is to build each architecture on its **native runner**,
exactly as the CLI binary matrix already does (`ubuntu-24.04` and
`ubuntu-24.04-arm` are both GitHub-hosted), push per-arch digests, and stitch a
manifest list with `docker buildx imagetools create`. The binary matrix in the
same workflow is the working precedent, so this is a known shape rather than an
experiment.

The two arch builds run in parallel, so wall-clock should be close to
unchanged.

## Status Updates

**2026-09-23 — filed, with evidence.** Found by [[KAIROS-T-0174]] while trying
to *execute* `tutorials/deploy-to-kubernetes.md` rather than write it from
reasoning. The tutorial contract in [[KAIROS-S-0008]] requires that every step
work every time for every learner (T6), which meant actually standing up a
cluster — and the lesson could not be completed at all.

That is the useful part: the gap had been recorded at release time as an
accepted limitation, and only survived contact with someone following the
documented path. **The Kubernetes tutorial is deferred to
[[KAIROS-T-0181]], blocked on this.**
