---
title: "Publish the docs site of a repository"
---

Kairos can serve the built static site of a repository, for example an
mdBook or a Starlight book. The CI of the repository builds the site and
sends it to Kairos. Kairos keeps it and serves it at a path of its own.
Each person on the network can read the site, also with no login.

**Before you start:**

- The deployment has a site store. The operator sets `KAIROS_PAGES_STORE` to
  `disk` (with `KAIROS_PAGES_DIR`) or to `s3` (with `KAIROS_PAGES_S3_BUCKET`).
  The compose example has a disk store.
- A service account for the CI. Make an API key for it: see
  [Give an agent machine access](/how-to/give-an-agent-machine-access/).
- The capability `pages` for the service account, on the delivery board of
  the team that owns the repository. An organization admin gives it, with the
  user id (UUID) of the service account:

  ```sh
  kairos grants add --board platform-delivery --capability pages --user <USER_ID>
  ```

## 1. Turn the site on

The site of a repository is off until you turn it on. A person who can change
the repository does this once:

```sh
kairos pages on --repo payments
```

You can also do this on the page **Admin, Repositories**: set **Site** to
`on`, then click **Save**.

## 2. Get the base path

Kairos serves the site at a fixed path. The build must use this path as its
base path, or the links of the site do not work:

```sh
kairos pages base-path --repo payments
# /pages/acme/platform/repos/payments/
```

The path has the organization, the owner team and the repository. If the
repository moves to another team, the path changes. Kairos continues to serve
the old site at the old path until CI publishes again.

## 3. Publish from CI

Add a job to the CI of the repository. This example builds an mdBook:

```sh
export KAIROS_URL=https://kairos.example.com
export KAIROS_KEY="$KAIROS_CI_KEY"   # the API key, from a CI secret
BASE=$(kairos pages base-path --repo payments)
MDBOOK_OUTPUT__HTML__SITE_URL="$BASE" mdbook build -d dist
kairos pages publish dist --repo payments
```

`kairos pages publish` takes the build folder or a `.tar.gz` of it. The
archive must have `index.html` at its root. The command sends the MD5 of the
archive, and Kairos refuses an archive that changed on the way.

Kairos refuses an archive that has:

- a symlink, a hard link, a device or a FIFO;
- an absolute path, a `..` part or a backslash;
- more than 1 GiB of files, or more than 200,000 files and folders. An
  operator can change these limits with `KAIROS_PAGES_MAX_SIZE` and
  `KAIROS_PAGES_MAX_FILES`.

The refusal gives the path or the limit.

## 4. Read the site

Open the base path in a browser. The team page in Kairos links to the site,
and shows when the last publish was and who sent it.

Kairos serves each page of the site in a sandbox. A script of the site cannot
read the session of the reader, and cannot use the API as the reader. Thus a
site cannot use `localStorage`. A search or a theme switch that uses
`localStorage` does not keep its state.

## Unpublish the site

```sh
kairos pages unpublish --repo payments --confirm
```

Kairos deletes the files of the site. To stop the serving and keep the files,
turn the site off:

```sh
kairos pages off --repo payments
```
