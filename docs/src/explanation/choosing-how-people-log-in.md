# Choosing how people log in

Kairos gives you three ways to let people in, and they are not three flavours of
the same thing. One of them makes Kairos responsible for your users' passwords.
That is the decision this page is about.

| | You run | Kairos stores | Good for |
|---|---|---|---|
| **Your own OIDC issuer** | an identity provider you already have | nothing | any organisation with an IdP |
| **The chart's bundled Dex** | nothing extra, but it is inside your release | nothing | evaluating Kairos |
| **Local accounts** | nothing at all | password hashes | small teams, and break-glass access |

The short version: **if your organisation has an identity provider, use it.** Not
because the alternatives are broken, but because every reason you already have an
IdP — one place to offboard someone, one place MFA is enforced, one audit trail —
is a reason not to keep a second set of credentials somewhere else.

## Bring your own issuer

This is the intended shape, and it is what [KAIROS-A-0016](https://github.com/colliery-io/kairos)
decided. You set `OIDC_ISSUER_URL` and `OIDC_AUDIENCE`, register Kairos as an OAuth
application at your IdP, and Kairos validates the tokens your IdP issues. It never
sees a password.

What you get from it is the whole reason IdPs exist. Someone leaves and you disable
one account, in the place your offboarding checklist already points at. MFA is your
IdP's problem and it is already solved there. Conditional access, device trust,
session policy — all of it keeps working, because Kairos is just another
application behind it.

Add [SCIM](../how-to/provision-users-with-scim.md) and the lifecycle arrives on its
own: a person's Kairos membership is revoked when your IdP says they have left,
rather than when their token happens to expire.

**Choose this if there is anybody in your organisation whose job includes the word
"security".** They will ask where the passwords are, and "our IdP" is the answer
that ends the conversation.

## The bundled Dex

`helm install` with no issuer named stands up a [Dex](https://dexidp.io) inside your
release, serves it through the same ingress under `/dex`, and points Kairos at it.
You get a working login without registering anything anywhere.

It is **for evaluation**, and the chart says so everywhere it appears. The reasons
are worse than the bundled database's:

- One replica with in-memory storage. Every restart invalidates every token and
  signs with fresh keys.
- Static users, listed in your values file.
- A password hash in your values file, and therefore in your release history.
- No user lifecycle, no password reset, no MFA, no audit trail.

It exists so that you can see Kairos working in ten minutes rather than an
afternoon. Point `config.oidc.issuerUrl` at something real before anyone who is not
you logs in.

## Local accounts

Set `KAIROS_LOCAL_AUTH=true` and Kairos authenticates people itself: an email, a
password, and a session bearer. An org admin creates the accounts; there is no
self-service sign-up.

This is the newest of the three and the one with a real cost, so here is what it
buys and what it costs.

### What it is for

**A small team with no IdP.** Five people who want to run Kairos on one box should
not have to stand up an identity provider first. With local accounts they do not:
no issuer, no ingress, no DNS, no TLS termination for a login redirect — just
Kairos and Postgres.

**Break-glass access on a deployment that does have an issuer.** Local accounts are
*additive*: you can run both. One local admin account means an issuer outage, an
expired signing certificate or a misconfigured audience does not lock you out of
your own deployment. This is a genuinely good reason to turn it on even in an
organisation with a mature IdP — and it is one account, not a parallel user
directory.

### What it costs

Kairos stores your users' passwords. Everything below follows from that sentence.

- **A password database to protect.** Hashes are argon2id at OWASP's server
  parameters, which is the right answer today and a parameter that has to be
  maintained as hardware gets faster. If the database leaks, how long those hashes
  hold is a decision this project made on your behalf.
- **No MFA.** There is no second factor and no plan for one. An IdP gives you this
  for free; local accounts do not give it to you at all.
- **No offboarding hook.** When someone leaves, an admin has to remember to remove
  them. SCIM cannot help, because SCIM is your IdP telling Kairos about a person and
  there is no IdP.
- **No reset email.** There is no mail subsystem, deliberately. A forgotten password
  is recovered by an org admin, or by an operator with database access. That is
  workable for ten people and unworkable for two hundred.
- **Sessions that do not survive a reload.** The browser holds the bearer in memory
  only, so reloading the page means logging in again. That is the cost of not
  writing a working API credential into browser storage.

### The line

Local accounts are right while *one person can remember who should have access*.
Past that, the things an IdP does — offboarding, MFA, one audit trail — stop being
conveniences and become the reason you have one. The switch is additive and
reversible: turn on the issuer, ask people to sign in through it, and take the
passwords away with
`kairos-server set-password` no longer being needed. The local admin can stay for
break-glass.

## What Kairos does to keep local passwords safe

Worth knowing, because it tells you what to preserve if you change any of it.

**Passwords are hashed with argon2id, not SHA-256.** Session tokens *are*
SHA-256, and the difference is the point: a session token is 32 random bytes, so
hashing it only needs to be one-way and fast, because it happens on every request.
A password is whatever a person chose, so the hash itself has to be expensive.
Making the two consistent would be a mistake in one direction or the other.

**Every login failure looks the same.** A wrong password, an unknown email, and an
account that authenticates through your issuer and has no password all return one
401 with one message — and take about the same time, because the unknown-email path
deliberately does the same argon2 work. Otherwise the response time would answer the
question the message refuses to, and an attacker could sort your real email
addresses from their guesses.

**Failed attempts are throttled**, per account and per source address, with a
lockout that decays in a minute. Short on purpose: a long lockout is a
denial-of-service someone can aim at an account whose name they know, and it turns
one person's typo into a support request.

**Changing a password ends every session that person held.** Enforced in the
storage layer, in the same transaction as the password write, because a password
change after a suspected compromise that left the attacker logged in would defeat
the only thing the person was trying to do.

**The first-boot admin is single-use.** `KAIROS_BOOTSTRAP_ADMIN` is consumed on a
boot that finds no users, and does nothing ever after — not even with the same
email. An environment variable in a values file is a permanent credential in a
release history, and a bootstrap that silently kept working would be a backdoor
with a documented name.

## Related

- [Turn on local accounts](../how-to/use-local-accounts.md) — the procedure
- [Configure an OIDC issuer](../how-to/configure-an-oidc-issuer.md) — the other path
- [Provision users with SCIM](../how-to/provision-users-with-scim.md) — lifecycle, if you have an IdP
- [Configuration](../reference/configuration.md) — every variable named here
