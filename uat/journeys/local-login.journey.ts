// J24 — "A deployment lets people in without an identity provider"
// (KAIROS-T-0206, KAIROS-I-0018).
//
// The initiative in one story: an org admin creates an account for a colleague who
// has never signed in anywhere, the colleague signs in with a password, and the admin
// can end that session when they need to.
//
// WHAT THIS JOURNEY DOES NOT COVER, and why. The arc in KAIROS-T-0206 begins with an
// operator standing up a deployment that has NO identity provider and bootstrapping
// the first admin. That cannot happen here: the UAT tier runs against one long-lived
// compose deployment which has a Dex, and the bootstrap is single-use on an EMPTY
// database by design — a journey that needed a second, fresh deployment would be a
// harness change rather than a journey. Those two steps are covered where they can be
// asserted properly:
//
//   * `kairos-db/tests/local_auth.rs::the_bootstrap_admin_is_single_use` — the
//     bootstrap runs once on an empty database and is inert afterwards, including with
//     the same email.
//   * `kairos-server/tests/local_login.rs::a_deployment_with_no_issuer_still_lets_people_in`
//     — a deployment with no issuer boots, `/api/login` works, and `/api/config` says
//     there is no issuer.
//
// The deployment under test has local auth on ALONGSIDE its Dex, which is the additive
// shape KAIROS-I-0018 chose and the one an organisation with an IdP would actually run
// to keep a break-glass admin.
import { expect } from '@playwright/test';
import { Api } from '../surfaces/api';
import { named, runContext } from '../run/context';
import { journey, step } from '../run/narrate';

/** A password long enough for the 12-character floor, and unique per run. */
const password = (label: string) => `uat-${label}-${Math.random().toString(36).slice(2, 10)}-pw`;

/** `POST /api/login`, as an unauthenticated caller — no bearer to hold yet. */
async function login(email: string, pw: string): Promise<{ status: number; body: any }> {
  const ctx = runContext();
  const res = await fetch(`${ctx.server}/api/login`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ email, password: pw }),
  });
  const text = await res.text();
  return { status: res.status, body: text ? JSON.parse(text) : null };
}

journey(
  'local-login',
  'A deployment lets people in without an identity provider',
  { humans: ['alice'] },
  async ({ cast, ledger }) => {
    const alice = cast.human('alice');
    const colleague = `${named('colleague').replace(/[^a-z0-9-]/g, '-')}@example.test`;
    const firstPassword = password('first');
    const resetPassword = password('reset');
    let userId = '';
    let sessionToken = '';

    await step(alice, 'checks the deployment offers a password login as well as its issuer', async () => {
      const ctx = runContext();
      const res = await fetch(`${ctx.server}/api/config`);
      const config = await res.json();
      // Both, not one: an organisation with an IdP keeps it and adds local accounts
      // for break-glass. This is the state the login page has to render.
      expect(config.local_auth, 'local accounts are on').toBe(true);
      expect(config.issuer, 'and the issuer is still there').toBeTruthy();
      return { local_auth: config.local_auth, issuer: config.issuer };
    });

    await step(alice, 'creates a local account for a colleague who has never signed in anywhere', async () => {
      const api = await alice.api();
      const created = await api.post('/api/local-accounts', {
        email: colleague,
        display_name: 'A Colleague',
        password: firstPassword,
        role: 'member',
      });
      userId = created.user_id;
      // Membership comes with the account, deliberately: one that belongs to no
      // organisation can sign in and then see nothing.
      expect(created.created, 'a new person').toBe(true);
      expect(created.membership_added, 'and a member of this org').toBe(true);
      ledger.add({
        kind: 'local account',
        label: colleague,
        delete: async () => {
          // No delete endpoint for a person; ending their sessions is what can be
          // undone from here, and the account is inert without a session.
          await (await alice.api()).raw('DELETE', `/api/local-accounts/${userId}/sessions`);
        },
      });
      return { email: created.email, user_id: created.user_id, created: created.created };
    });

    await step(alice, 'is refused a password that is too short', async () => {
      const api = await alice.api();
      const res = await api.raw('POST', '/api/local-accounts', {
        email: `short-${colleague}`,
        password: 'short',
      });
      expect(res.status).toBe(422);
      expect(res.body.error.code).toBe('WEAK_PASSWORD');
      return { status: res.status, code: res.body.error.code };
    });

    await step(alice, 'watches a wrong password be refused without saying why', async () => {
      const wrong = await login(colleague, 'not-the-password-at-all');
      const unknown = await login(`nobody-${colleague}`, firstPassword);
      expect(wrong.status).toBe(401);
      expect(unknown.status).toBe(401);
      // The property, not a nicety: a wrong password and an unknown email must be
      // indistinguishable, or the endpoint sorts real addresses from guesses.
      expect(unknown.body).toEqual(wrong.body);
      return { status: wrong.status, message: wrong.body.error.message };
    });

    await step(alice, 'sees the colleague sign in with the password and reach their work', async () => {
      const signedIn = await login(colleague, firstPassword);
      expect(signedIn.status).toBe(200);
      sessionToken = signedIn.body.token;
      expect(sessionToken.startsWith('kairos_ss_'), 'an opaque session bearer').toBe(true);

      // The session is a bearer like any other: the tenant comes from the request,
      // not from the token, because a session stands in for an OIDC token.
      const theirs = new Api(sessionToken);
      const me = await theirs.whoami();
      expect(me.user.email).toBe(colleague);
      const boards = await theirs.boards();
      expect(boards.length, 'and they can see the org’s boards').toBeGreaterThan(0);
      return { email: me.user.email, boards: boards.length, expires_at: signedIn.body.expires_at };
    });

    await step(alice, 'lists the colleague’s sessions and sees one, without seeing the token', async () => {
      const api = await alice.api();
      const listed = await api.get(`/api/local-accounts/${userId}/sessions`);
      const active = listed.items.filter((s: any) => s.active);
      expect(active.length).toBe(1);
      expect(JSON.stringify(listed)).not.toContain('kairos_ss_');
      return { total: listed.total, active: active.length };
    });

    await step(alice, 'resets the password, which ends the session the colleague was using', async () => {
      const api = await alice.api();
      await api.put(`/api/local-accounts/${userId}/password`, { password: resetPassword });

      // The reason a reset exists is a suspected compromise. Leaving the old session
      // working would defeat it.
      const stale = await new Api(sessionToken).raw('GET', '/api/whoami');
      expect(stale.status).toBe(401);

      const again = await login(colleague, resetPassword);
      expect(again.status).toBe(200);
      sessionToken = again.body.token;
      return { old_session: stale.status, signed_in_again: again.status };
    });

    await step(alice, 'ends every session the colleague holds, without changing their password', async () => {
      const api = await alice.api();
      await api.delete(`/api/local-accounts/${userId}/sessions`);
      const stale = await new Api(sessionToken).raw('GET', '/api/whoami');
      expect(stale.status).toBe(401);

      // Separate operations on purpose: "sign them out everywhere" and "they forgot
      // their password" are different incidents. The password still works.
      const back = await login(colleague, resetPassword);
      expect(back.status).toBe(200);
      return { sessions_ended: true, password_still_works: back.status === 200 };
    });

    await step(alice, 'signs the colleague out for good and confirms the bearer is dead', async () => {
      const ctx = runContext();
      const res = await fetch(`${ctx.server}/api/logout`, {
        method: 'POST',
        headers: { authorization: `Bearer ${sessionToken}` },
      });
      expect(res.status).toBe(204);
      const after = await new Api(sessionToken).raw('GET', '/api/whoami');
      expect(after.status).toBe(401);
      return { logout: res.status, after: after.status };
    });
  },
);
