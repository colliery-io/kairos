// Real PKCE token minting for the competing-writer API calls (KAIROS-T-0045).
//
// The smoke test's *browser* login is a genuine in-browser Authorization Code
// + PKCE flow driven through the Dex login form. Separately, the WS live-update
// step and the 409 conflict step need a SECOND writer acting concurrently
// "via API" (per the acceptance criteria). Rather than crack a token out of the
// SPA's in-memory session, this helper runs the identical PKCE flow headlessly
// against Dex (generate verifier/S256 challenge → walk the authorize → login →
// callback chain → exchange the code through the server's same-origin relay)
// and returns the access token. It uses Node's global fetch with a private
// cookie jar so it is fully independent of the browser context under test.

import crypto from 'node:crypto';

const b64url = (b: Buffer) =>
  b.toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');

export interface MintOptions {
  issuer?: string;
  server?: string;
  clientId?: string;
  email?: string;
  password?: string;
}

/** Run a full headless PKCE login and return the access token. */
export async function mintToken(opts: MintOptions = {}): Promise<string> {
  const issuer = opts.issuer ?? process.env.E2E_ISSUER ?? 'http://localhost:41558/dex';
  const server = opts.server ?? process.env.E2E_GUI_BASE_URL ?? 'http://localhost:41080';
  const clientId = opts.clientId ?? 'kairos-web';
  const redirect = `${server}/callback`;
  const email = opts.email ?? 'alice@kairos.test';
  const password = opts.password ?? 'alice-password';

  const verifier = b64url(crypto.randomBytes(32));
  const challenge = b64url(crypto.createHash('sha256').update(verifier).digest());
  const state = b64url(crypto.randomBytes(16));

  const jar: Record<string, string> = {};
  const remember = (res: Response) => {
    const setCookie = (res.headers as any).getSetCookie?.() ?? [];
    for (const c of setCookie as string[]) {
      const [kv] = c.split(';');
      const i = kv.indexOf('=');
      jar[kv.slice(0, i)] = kv.slice(i + 1);
    }
  };
  const cookieHeader = () =>
    Object.entries(jar).map(([k, v]) => `${k}=${v}`).join('; ');
  const follow = async (url: string, init: RequestInit = {}) => {
    const res = await fetch(url, {
      ...init,
      redirect: 'manual',
      headers: { ...(init.headers as any), cookie: cookieHeader() },
    });
    remember(res);
    return res;
  };

  // authorize → (302 chain) → the Dex login form
  const authUrl =
    `${issuer}/auth?` +
    new URLSearchParams({
      client_id: clientId,
      response_type: 'code',
      scope: 'openid email profile offline_access',
      redirect_uri: redirect,
      state,
      code_challenge: challenge,
      code_challenge_method: 'S256',
    }).toString();

  let res = await follow(authUrl);
  let cur = new URL(res.headers.get('location')!, issuer).toString();
  let html = '';
  for (let i = 0; i < 6; i++) {
    res = await follow(cur);
    if (res.status >= 300 && res.status < 400) {
      cur = new URL(res.headers.get('location')!, cur).toString();
      continue;
    }
    html = await res.text();
    break;
  }

  // POST the credentials to the login form's action
  const action = html.match(/action="([^"]+)"/)![1].replace(/&amp;/g, '&');
  res = await follow(new URL(action, cur).toString(), {
    method: 'POST',
    headers: { 'content-type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({ login: email, password }).toString(),
  });
  cur = new URL(res.headers.get('location')!, cur).toString();

  // walk to the /callback redirect and read the authorization code
  for (let i = 0; i < 6 && new URL(cur).pathname !== '/callback'; i++) {
    res = await follow(cur);
    cur = new URL(res.headers.get('location')!, cur).toString();
  }
  const callback = new URL(cur);
  const code = callback.searchParams.get('code');
  if (!code) throw new Error(`PKCE login did not yield a code (landed at ${cur})`);
  if (callback.searchParams.get('state') !== state) {
    throw new Error('PKCE state mismatch on callback');
  }

  // exchange through the server's same-origin token relay
  const tokenRes = await fetch(`${server}/api/auth/token`, {
    method: 'POST',
    headers: { 'content-type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({
      grant_type: 'authorization_code',
      code,
      redirect_uri: redirect,
      code_verifier: verifier,
    }).toString(),
  });
  if (!tokenRes.ok) {
    throw new Error(`token relay ${tokenRes.status}: ${await tokenRes.text()}`);
  }
  const token = (await tokenRes.json()).access_token as string | undefined;
  if (!token) throw new Error('token relay returned no access_token');
  return token;
}
