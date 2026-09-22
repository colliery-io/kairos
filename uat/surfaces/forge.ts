// Signed GitHub webhook deliveries (ported from e2e/helpers/api.ts,
// KAIROS-T-0102): what "a PR was opened / merged in the repository" looks
// like to Kairos. The connection's secret is returned once at creation,
// which is exactly why the journey creates the connection itself.
import crypto from 'node:crypto';
import { runContext } from '../run/context';
import type { Api } from './api';

export interface ForgeConnection {
  id: string;
  webhookUrl: string;
  webhookSecret: string;
}

/** Connect webhooks for a registered repository (slug or UUID). */
export async function createForgeConnection(api: Api, repository: string): Promise<ForgeConnection> {
  const body = await api.post('/api/forge-connections', { repository });
  return { id: body.id, webhookUrl: body.webhook_url, webhookSecret: body.webhook_secret };
}

/** Deliver a signed `X-GitHub-Event` payload; returns the HTTP status. */
export async function deliverGithubWebhook(
  connection: ForgeConnection,
  event: string,
  payload: unknown,
): Promise<number> {
  const body = JSON.stringify(payload);
  const signature =
    'sha256=' + crypto.createHmac('sha256', connection.webhookSecret).update(body).digest('hex');
  // The delivery URL names the deployment's PUBLIC url; deliver to the same
  // path on the server under test.
  const path = new URL(connection.webhookUrl).pathname;
  const res = await fetch(`${runContext().server}${path}`, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
      'x-github-event': event,
      'x-hub-signature-256': signature,
    },
    body,
  });
  return res.status;
}

/** A GitHub `pull_request` payload naming `code` in its title and branch. */
export function githubPullRequest(opts: {
  number: number;
  code: string;
  repoFullName: string;
  state: 'open' | 'closed';
  merged?: boolean;
  updatedAt: string;
  title?: string;
  author?: string;
}): unknown {
  const author = opts.author ?? 'uat-agent';
  return {
    action: opts.state === 'closed' ? 'closed' : 'opened',
    repository: {
      full_name: opts.repoFullName,
      html_url: `https://github.com/${opts.repoFullName}`,
    },
    sender: { login: author },
    pull_request: {
      number: opts.number,
      state: opts.state,
      merged: opts.merged ?? false,
      draft: false,
      title: opts.title ?? `Work on ${opts.code}`,
      body: '',
      html_url: `https://github.com/${opts.repoFullName}/pull/${opts.number}`,
      updated_at: opts.updatedAt,
      user: { login: author },
      head: { ref: `${author}/${opts.code}-branch` },
    },
  };
}
