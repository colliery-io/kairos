// Headless PKCE login (ported from e2e/helpers/auth.ts, KAIROS-T-0045):
// walk the issuer's authorize → login form → callback chain with a private
// cookie jar and exchange the code through the server's same-origin relay.
// Works against Dex (compose) and any issuer with a password login form
// whose action can be read from the HTML.
//
// Dex keeps ONE refresh token per user+client: mint every human persona's
// token BEFORE that persona logs into a browser, or the browser's silent
// restore is invalidated and full navigations bounce to the login page.
import crypto from 'node:crypto';

const b64url = (b: Buffer) =>
  b.toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');

export interface MintOptions {
  issuer: string;
  server: string;
  email: string;
  password: string;
  clientId?: string;
}

export async function mintToken(opts: MintOptions): Promise<string> {
  const { issuer, server, email, password } = opts;
  const clientId = opts.clientId ?? 'kairos-web';
  const redirect = `${server}/callback`;
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
  const cookieHeader = () => Object.entries(jar).map(([k, v]) => `${k}=${v}`).join('; ');
  const follow = async (url: string, init: RequestInit = {}) => {
    const res = await fetch(url, {
      ...init,
      redirect: 'manual',
      headers: { ...(init.headers as any), cookie: cookieHeader() },
    });
    remember(res);
    return res;
  };

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

  const action = html.match(/action="([^"]+)"/)?.[1].replace(/&amp;/g, '&');
  if (!action) throw new Error(`no login form found at ${cur} for ${email}`);
  res = await follow(new URL(action, cur).toString(), {
    method: 'POST',
    headers: { 'content-type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({ login: email, password }).toString(),
  });
  const next = res.headers.get('location');
  if (!next) throw new Error(`login for ${email} did not redirect (status ${res.status}) — wrong password?`);
  cur = new URL(next, cur).toString();

  for (let i = 0; i < 6 && new URL(cur).pathname !== '/callback'; i++) {
    res = await follow(cur);
    cur = new URL(res.headers.get('location')!, cur).toString();
  }
  const callback = new URL(cur);
  const code = callback.searchParams.get('code');
  if (!code) throw new Error(`PKCE login did not yield a code (landed at ${cur})`);
  if (callback.searchParams.get('state') !== state) throw new Error('PKCE state mismatch');

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
  if (!tokenRes.ok) throw new Error(`token relay ${tokenRes.status}: ${await tokenRes.text()}`);
  const token = (await tokenRes.json()).access_token as string | undefined;
  if (!token) throw new Error('token relay returned no access_token');
  return token;
}
