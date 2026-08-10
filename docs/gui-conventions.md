# Kairos GUI conventions (`crates/kairos-web`)

Established by KAIROS-T-0039; **binding for the GUI fan-out tasks
(KAIROS-T-0040 boards, T-0041 item detail, T-0042 search, T-0043 admin,
T-0044 activity) and everything after them.** Architecture decisions come
from KAIROS-A-0015 (Leptos CSR, aurora-dark, served at `/`), A-0010
(PKCE), and A-0013 (single artifact); this document is the how.

Read `aurora-dark`'s `PATTERNS.md` (rustdoc: `cargo doc -p
colliery-io-aurora`; source repo: github.com/colliery-io/aurora-dark)
before building a screen — it is the pick-by-intent component guide this
document builds on.

---

## 1. Build and serve

- **Toolchain: trunk** (`Trunk.toml` in the crate). Chosen over
  cargo-leptos deliberately: cargo-leptos earns its keep for SSR/hydrate
  dual-target builds; A-0015 fixes CSR-only, for which trunk is the
  standard Leptos 0.8 path (the aurora gallery uses it too). Entry point:

  ```
  angreal web build            # dev bundle → crates/kairos-web/dist/
  angreal web build --release  # optimized bundle (opt-level z + wasm-opt)
  ```

  The task provisions the `wasm32-unknown-unknown` target and trunk
  itself (pinned `cargo install` into `target/tools/` if not on PATH —
  never Homebrew).

- **Serving** (`kairos-server::web`, mounted as the router fallback):
  - **dev**: `KAIROS_WEB_DIST=crates/kairos-web/dist` on the server env.
    Iterate with `trunk watch` in `crates/kairos-web/` + a running
    server; reload the browser. (`trunk serve` is NOT the flow — the SPA
    needs its same-origin `/api`.)
  - **release (what ships, A-0013)**: assets are **embedded in the
    binary** via the server's off-by-default `embed-web` cargo feature:
    `angreal web build --release`, then `cargo build -p kairos-server
    --release --features embed-web`. Plain `cargo build` never needs
    wasm tooling; without dist/embed the server serves a placeholder
    page at `/` that says how to build the GUI.
  - SPA fallback semantics: asset paths serve the asset; route-like
    paths serve `index.html`; `/api`, `/mcp`, `/scim`, `/ws`,
    `/healthz`, `/readyz`, `/metrics`, `/.well-known` are **reserved**
    and 404 with the S-0005 envelope. Trunk content-hashes asset names,
    so assets ship `Cache-Control: immutable`; `index.html` is
    `no-cache`.

- **Host builds stay green**: the whole app lives in the crate's lib
  target, so `cargo build/clippy/test --workspace` compiles it natively
  (browser APIs are compile-time fine, runtime-wasm-only). Pure helpers
  (encoders, mirrors) get host unit tests.

### Dev loop against the compose stack

```sh
angreal services up                      # postgres + dex
angreal web build                        # or: trunk watch, in a second shell
angreal db seed                          # demo tenant fixture
DATABASE_URL=postgres://kairos:kairos@localhost:5432/kairos \
  OIDC_ISSUER_URL=http://localhost:5558/dex \
  OIDC_AUDIENCE=kairos-web \
  KAIROS_SINGLE_TENANT=demo \
  KAIROS_WEB_DIST=crates/kairos-web/dist \
  KAIROS_LOG_FORMAT=pretty \
  cargo run -p kairos-server -- serve
# → http://localhost:8080  (alice@kairos.test / alice-password)
```

Two dev-stack facts worth knowing (both are deployment config, not code):

- **`OIDC_AUDIENCE=kairos-web` for GUI work.** Dex stamps `aud` with the
  requesting client id, and the GUI's client is `kairos-web`
  (`.angreal/dex/config.yaml`). Browse via `localhost`, not `127.0.0.1`.
  API/CLI-focused work uses `kairos-cli` tokens instead — a server
  accepts one audience, pick per what you're exercising. Production IdPs
  put one deployment-wide audience on all first-party clients (A-0010),
  so this fork is dev-only.
- **Any localhost port 8080–8099 works for PKCE (KAIROS-T-0050).** The
  `kairos-web` client registers `http://localhost:{8080..8099}/callback`,
  so several `kairos-server` instances can each do a real browser PKCE
  login on their own port concurrently. Dex v2.43.1 exact-matches the
  redirect URI (verified: an out-of-range port is rejected at the
  approval step, not at authorize), so stay inside the range. This
  retires the old CDP Fetch-interception / port-queue workarounds the
  early GUI wave needed; use a real login on a port in the range. Note:
  the shared dev Dex must be restarted (`angreal services reset`) after
  a config change for a new range to take effect.
- **`KAIROS_SINGLE_TENANT=demo`** pins the tenant — a browser on
  `localhost` has no tenant subdomain and sends no `X-Tenant`.

## 2. Design system: aurora-dark, tokens only

`aurora-dark` (crates.io: `colliery-io-aurora`, imported as
`aurora_dark`) is the design system. The stylesheet is injected at
runtime by `<AuroraStyles/>` once, at the app root (CSR has no
first-paint-flash concern worth a build hook).

**The token rule.** No raw color literals anywhere in this crate — not
in Rust, not in CSS, not in inline `style=`, not in SVG. Color arrives
two ways only:

- Rust props / data-driven color: `aurora_dark::tokens::token::*`
  constants (`token::ICE`, …) and `status_color(...)`;
- CSS (including inline styles and SVG): `var(--ice)`, `var(--panel)`,
  … For SVG, put the var in `style="stroke:var(--ice);"` — presentation
  attributes don't resolve CSS variables.

Mechanized as **`angreal web lint`** (a CI gate): greps
`crates/kairos-web` (`src/**/*.rs`, `*.css`, `index.html`) for
`#hex` / `rgb()` / `hsl()` literals. Keep it clean; do not add an
allowlist — if a color feels missing, it's a design-token conversation,
not a literal.

Component choice: consult PATTERNS.md's "Pick by intent" table before
writing markup. App-specific chrome (nav links, full-viewport layout)
lives in `app.css`, uses `.kairos-*` class names, and consumes tokens.
Never restyle `.cl-*` classes except where `app.css` already does
(the appshell full-viewport override).

## 3. Module layout and routes

```
crates/kairos-web/
  index.html        trunk target (app.css link; wasm-opt config)
  app.css           Kairos-specific layout chrome (tokens only)
  src/
    main.rs         wasm entry (mounts App; keep 5 lines)
    lib.rs          module map
    app.rs          router, protected Shell, nav, whoami, guard
    auth.rs         PKCE + session (owned by T-0039; extend, don't fork)
    api.rs          fetch layer + mirror DTOs
    pages.rs        one *Page component per route
```

Routes (in `app.rs`): `/login`, `/callback` (auth, public);
`/boards`, `/boards/:board` (T-0040), `/items/:code` (T-0041),
`/search` (T-0042), `/admin` (T-0043), `/activity` (T-0044) — all under
the protected `Shell` `ParentRoute`; `/` redirects to `/boards`.

Fan-out rules:

- Replace your stub in `pages.rs`; when a page outgrows a screenful,
  move it to `src/pages/<name>.rs` (`pages` becomes a directory module)
  but **keep the `pages::XxxPage` export stable** so `app.rs` only ever
  changes to add new routes.
- Add sub-routes under your own path segment only (`/boards/…` belongs
  to T-0040, etc.); register them in `app.rs`'s route table and keep its
  module-doc route map current.
- Nav changes (new top-level sections) belong to the task that owns the
  section; the nav is `Shell`'s `navbar` in `app.rs` (a `NavLink` per
  entry — `aria-current` styling comes free).

## 4. Data layer

**Decision (T-0039): `kairos-client` is not used in the GUI.** It does
not compile for `wasm32-unknown-unknown` (native `tokio`/
`tokio-tungstenite` transports). Per A-0015's escape hatch, the GUI
talks HTTP itself:

- **Transport: `gloo-net`** via `api::get_json` (add `post_json` /
  `patch_json` / `delete_json` siblings in `api.rs` as your task needs
  them — same shape: bearer header, envelope-aware error mapping, 401
  hook).
- **Mirror DTOs**: local serde structs with the exact wire field names,
  *partial on purpose* (declare only the fields the view reads; serde
  ignores the rest). Every mirror carries a `/// mirror of:
  `kairos_client::types::X`` doc line so drift is greppable; add a
  decode test against a realistic JSON body (see
  `api::tests::whoami_mirror_decodes_server_shape`).
- **Follow-up on record** (T-0039 status updates): extract the
  plain-serde types from `kairos-client` into a `kairos-types` crate
  both native and wasm consumers use. When that lands, mirrors get
  replaced wholesale; until then, do not "fix" kairos-client from a GUI
  task.
- **Resources**: components own no fetch logic. Wrap `api::*` calls in
  `LocalResource::new(move || { let _ = auth.token(); api::foo(auth, …) })`
  — reading `auth.token()` (or your params signal) in the closure's
  sync part is what makes the resource refetch. Server state is the
  source of truth: after a mutation, refetch (or apply the `/ws/events`
  message, T-0040); no client-side cache invalidation cleverness (A-0015).
- **Errors are `aurora_dark::tokens::ApiError`** end to end — `api.rs`
  maps the S-0005 envelope onto it in one place, and `<ErrorState/>`
  renders it. Components never inspect HTTP statuses; a 401 is handled
  globally (session cleared → login redirect).
- Live updates (T-0040+): subscribe to `/ws/events` (browser token
  handling documented in `kairos-server::ws`), re-fetch changed items on
  event — don't patch local state from event payloads.

## 5. Async view states

Every async view renders all four states — the aurora shape, verbatim:

```rust
{move || match resource.get() {
    None => view! { <Loading/> }.into_any(),
    Some(Err(error)) => view! { <ErrorState error on_retry/> }.into_any(),
    Some(Ok(items)) if items.is_empty() =>
        view! { <Empty message="Nothing here yet."/> }.into_any(),
    Some(Ok(items)) => /* render */,
}}
```

- `Loading` for a page/panel; a dimmed `Text`/`Loader` for something
  inline (the whoami badge pattern in `app.rs`).
- `ErrorState` (with `on_retry` re-triggering the resource) for failed
  loads; `Alert` for section-scoped notices; `Banner` for page-level
  transient ones. Never a blank region, never a raw error string.
- `Empty` always says what would fill it ("No boards yet — create one
  from Admin"), not just "empty".

## 6. Auth (PKCE) — owned by `auth.rs`

Decisions (T-0039), with the full flow documented in `auth.rs`:

- **Discovery = `GET /api/config`** (public, same-origin; served by
  `kairos-server::web`): `{issuer, client_id, authorization_endpoint}`.
  Chosen over build-time env (one bundle must serve any deployment) and
  over RFC 9728 metadata (that answers "which authorization server",
  but not "which client id" — and the SPA still couldn't CORS-fetch the
  issuer's discovery document).
- **Token exchange goes through the same-origin relay `POST
  /api/auth/token`** (code exchange + refresh grant). IdP token
  endpoints — the dev Dex included — generally don't serve CORS to
  SPAs; the server forwards to the issuer verbatim (whitelisted params,
  configured client id) and holds no secret. PKCE protects the exchange
  end to end; A-0010's "no IdP on the request path" still holds (this
  is a login-event path only).
- The **access token** lives **in memory** (a reactive signal): no
  cookies, no localStorage (A-0015). The **refresh token** additionally
  sits in `sessionStorage` (T-0071, amending A-0015): per-tab and
  cleared on tab close, it lets a page reload restore the session via a
  silent refresh grant *before* the guard redirects — necessary because
  IdPs without an SSO session (the dev Dex password connector) turn the
  "silent" issuer redirect into a login form on every reload. Logout
  and any failed refresh remove it. The PKCE verifier/state live in
  `sessionStorage` only between redirect-out and callback.
- **Silent refresh**: timer at `expires_in − 60s` via the relay's
  refresh grant; failure clears the session (stored refresh token
  included).
- **Guard**: everything under `Shell` requires a session; without one it
  *redirects to the issuer* (A-0015), remembering the path. `/login`
  exists for logout landings and explicit sign-in. A 401 from any API
  call clears the session → same redirect. Logout is client-side only
  (drop tokens); no IdP logout round-trip in v1.
- The bearer sent to `/api` is the **access token** (Dex mints JWT
  access tokens with `aud`/`email`; the server validates them exactly
  like CLI tokens).

Fan-out tasks should never touch `auth.rs` beyond consuming
`use_auth()`; auth-shaped needs (e.g. roles for admin gating, T-0043)
come from `/api/whoami` data, not token introspection.

## 7. Component patterns

- One `#[component]` per meaningful unit; props over context, except
  the app-wide `Auth` (context via `use_auth()`).
- Inputs bind `RwSignal`s; handlers are `Callback`s (aurora components'
  own convention).
- Small closures + `view!` beats abstraction: no wrapper components
  around aurora primitives "for consistency" — the conventions grep and
  PATTERNS.md are the consistency mechanism.
- Text in views is quoted strings (`"Boards"`), not consts, until a
  string is used twice.
- `expect`/`unwrap` are forbidden in components (clippy allows them,
  reviews don't); the only sanctioned panic is `auth.rs`'s "no window"
  (impossible in a browser).

## 8. Testing and verification

- Pure logic (encoders, mirrors, guards' path math) gets host unit
  tests in-module (`cargo test -p kairos-web` runs them natively).
- Server-side serving/auth surface is covered in
  `crates/kairos-server/tests/web.rs` — extend it if you add server
  routes for your page (rare; talk to the API instead).
- Browser verification: each GUI task demonstrates its ACs against the
  compose stack (dev loop above) and records evidence in its status
  updates; the Playwright smoke tier lands with KAIROS-T-0045.
