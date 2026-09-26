---
id: the-compose-deployment-joins-a
level: task
title: "The compose deployment joins a tailnet and runs on local accounts alone"
short_code: "KAIROS-T-0209"
created_at: 2026-09-26T22:06:50.115685+00:00
updated_at: 2026-09-26T22:20:14.894194+00:00
parent: 
blocked_by: []
archived: false

tags:
  - "#task"
  - "#feature"
  - "#phase/completed"


exit_criteria_met: false
initiative_id: NULL
---

# The compose deployment joins a tailnet and runs on local accounts alone

## Objective

Run Kairos on Dylan's Mac, reachable only over the tailnet at
`https://kairos.<tailnet>.ts.net/`, the way skadi is deployed (skadi commits
346d9d9 and 32a38a7 in ../skadi). Everything that is not personal
infrastructure lands in `deploy/` and the book, so the compose stack gains a
second front door that any operator can choose.

Two things stood in the way of this being a config-only exercise:

1. `deploy/docker-compose.yaml` hard-required `OIDC_ISSUER_URL` and
   `OIDC_AUDIENCE` with `:?` even though [[KAIROS-T-0208]] made them optional
   in the server when `KAIROS_LOCAL_AUTH` is on. The compose stack could not
   express the exact deployment [[KAIROS-I-0018]] exists to enable.
2. Caddy was unconditional and published 80/443 on the host, which a
   tailnet-only deployment must not do.

## Backlog Item Details

### Type
- [x] Feature - New functionality or enhancement

### Priority
- [x] P1 - High (important for user experience)

### Business Justification
- **User Value**: A person with a tailnet and no IdP gets a private HTTPS Kairos with one auth key and one compose command.
- **Business Value**: The first real (dogfood) deployment of Kairos; exercises [[KAIROS-I-0018]] end to end.
- **Effort Estimate**: S

## Acceptance Criteria

- [x] `deploy/docker-compose.yaml` no longer refuses to start on local accounts alone: OIDC variables interpolate with `:-`, and the server's own boot check is the gate.
- [x] The front door is a compose profile: `COMPOSE_PROFILES=caddy` (public HTTPS on 80/443, the previous default) or `COMPOSE_PROFILES=tailscale` (tailnet only, no host ports).
- [x] The tailscale profile: a `tailscale` sidecar in the kairos network namespace with a declared `TS_SERVE_CONFIG` (443 to 127.0.0.1:8080, 80 to an nginx 301) — serve, never funnel.
- [x] `deploy/.env.example` documents both doors and the tailnet variables.
- [x] A how-to in the book (`docs/src/how-to/serve-on-a-tailnet.md`) that passes `angreal docs ste`.
- [x] `docker compose config` renders for the caddy profile, the tailscale profile, and neither.
- [x] Deployed on this Mac: `tailscale serve status` says "tailnet only", a Let's Encrypt cert is issued, `https://kairos.<tailnet>.ts.net/healthz` answers 200, and `http://` 301s to it.
- [x] Bootstrap admin created, single tenant provisioned, login works over the tailnet.

## Implementation Notes

### Technical Approach
Mirror skadi: `network_mode: "service:kairos"` for both sidecars, `tailscale-state` volume, `./tailscale:/config:ro` carrying `serve.json` with `${TS_CERT_DOMAIN}` so no tailnet name is in the repo. nginx:1.27-alpine listens on 8081 and returns 301 to `https://$host$request_uri`; needed because `tailscale serve` does not forward X-Forwarded-Proto (measured in skadi 2026-09-26), so :80 must land somewhere that is not kairos.

`KAIROS_TRUSTED_PROXY=true` stays correct: tailscale serve sends X-Forwarded-For, and kairos still only `expose`s 8080, so the header is never caller-supplied.

Tenant addressing on a tailnet: MagicDNS has no wildcard, so `KAIROS_SINGLE_TENANT` is the only workable shape.

### Dependencies
[[KAIROS-T-0208]] (issuer optional), [[KAIROS-T-0204]] (first-boot admin). Tailnet needs "HTTPS Certificates" enabled (already is: skadi has a cert).

### Risk Considerations
Moving Caddy behind a profile changes the default for an existing `.env` that has no `COMPOSE_PROFILES`: Caddy silently does not start. Loud (nothing on 443) and a one-line fix; documented in `.env.example` and the how-to.

## Status Updates

- 2026-09-26: created after reading skadi's deploy and the kairos reference stack. Plan above. Starting with the compose and serve config.
- 2026-09-26 (done): everything in `deploy/` and the book is written and verified. `docker compose config` renders under `caddy`, `tailscale` and no profile; the env file's `$argon2id` hash survives compose interpolation only when single-quoted (compose doubles `$` in `config` output, which is display, not mangling). `angreal docs ste` is at the baseline after rewriting three sentences in the new how-to.
- 2026-09-26 (deployed): stack up on the Mac with `COMPOSE_PROFILES=tailscale`, image 0.4.0, local auth only. The auth key from skadi's `.env` was reusable, so no new key was needed. `tailscale serve status` shows three `tailnet only` listeners; Let's Encrypt issued `CN=kairos.<tailnet>.ts.net` (expires 2026-12-25, auto-renews). Verified from skadi's tailnet node, because this Mac itself has no Tailscale client: `https://.../healthz` 200 with `SSL certificate verify ok`, `http://.../login` 301 to the https URL with the path kept, `/api/config` reports `local_auth: true` and no issuer. Nothing published on the host: 127.0.0.1:8080 and :8081 refuse.
- 2026-09-26 (identity): first-boot admin `dylan.storey@gmail.com` created on first boot; `POST /api/login` 200 over the tailnet. `create-tenant` via the server subcommand seeds no membership (the how-to now says so), so the empty tenant was dropped with `--confirm` and recreated through `POST /api/admin/tenants` as the bootstrap admin, which seeded the org-admin row. `/api/whoami` confirms `colliery` / `admin`. Bootstrap variables commented out in `deploy/.env`; the recreate that followed also recreated both sidecars, confirming the "compose does this for you" claim in the how-to.
- Measured: redirect nginx 6.6 MiB idle (matches skadi's 6.7), tailscale 22 MiB, kairos 176 MiB.
- Not done, by design: no commit (not asked). The Mac is not on the tailnet, so a browser check from this machine is impossible; a phone or the other Mac on the tailnet is the real test.