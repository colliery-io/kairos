// J10 — "A decision is made, then superseded" (KAIROS-I-0014).
//
// J2 creates one ADR from the CLI and never opens it again, so two things
// nothing walks: the ADR board's own lifecycle (Draft → Discussion →
// Decided → Superseded, a transition graph like any other board's) and
// the `supersedes` edge, which is the only relationship type whose whole
// purpose is to say "this used to be true".
//
// The story runs the arc: the team records a decision, argues it, decides
// it, hangs the work that implements it underneath — and months later
// (minutes here) replaces it. The replacement has to leave BOTH records
// readable and the link between them visible from either end, because an
// architecture decision nobody can trace backwards is just a wiki page.
import { expect, type Locator, type Page } from '@playwright/test';
import { named } from '../run/context';
import { journey, step } from '../run/narrate';
import { openBoard, openItem, panel } from '../surfaces/gui';
import { shortCodes } from '../surfaces/mcp';

/**
 * A column on a NON-delivery board. `gui.ts`'s `column()` scopes to the
 * planned lane and falls back to `<main>`; the ADR board has neither
 * (lanes are a delivery-board projection of `work_class`, and the app
 * shell has no `<main>`), so it needs its own locator.
 */
function adrColumn(page: Page, name: string): Locator {
  return page.locator('section.kairos-board__column', {
    has: page.locator('.kairos-board__column-head', { hasText: name }),
  });
}

/** The Relationships-panel group with a given label ("supersedes", "parent"). */
function relationshipGroup(page: Page, label: string): Locator {
  return panel(page, 'Relationships').locator('.kairos-relationships__group', { hasText: label });
}

journey(
  'decision-record',
  'A decision is made, and later superseded',
  { humans: ['alice', 'bob'] },
  async ({ cast, ledger }) => {
    const alice = cast.human('alice');
    const bob = cast.human('bob');
    const api = await alice.api();
    const adrBoard = await api.boardBySlug('adrs');
    const columnId = (name: string): string => {
      const found = adrBoard.columns.find((c: any) => c.name === name);
      if (!found) throw new Error(`the ADR board has no ${name} column`);
      return found.id;
    };
    // The initiative the decision is about: the first real (non-bucket)
    // one this deployment has, so the story never hard-codes a seed code.
    const initiatives = (await api.get('/api/initiatives?limit=100')).items ?? [];
    const initiative = initiatives.find((i: any) => !i.is_bucket)?.short_code as string;
    expect(initiative, 'the deployment has a non-bucket initiative').toBeTruthy();

    let decision = '';
    let replacement = '';
    let note = '';

    await step(bob, 'starts to write the decision down, and is told architecture decisions are not his to record', async () => {
      const mcp = await bob.mcp();
      // A team member can read every ADR and none of them is his to
      // write: the ADR board is org-level and gated on `manage_adrs`.
      const refused = await mcp.refused('create_item', {
        item_type: 'adr',
        board: 'adrs',
        title: named('decision: one queue for outbound email'),
        content: '## Decision\n\nOne outbound queue.\n',
      });
      expect(refused).toContain('FORBIDDEN');
      expect(refused).toContain('manage_adrs');
      return { who: bob.credentials.email, refused_on: 'adrs', needs: 'manage_adrs' };
    });

    await step(alice, 'writes down the decision the team is about to argue about', async () => {
      const mcp = await alice.mcp();
      const created = await mcp.call('create_item', {
        item_type: 'adr',
        board: 'adrs',
        title: named('decision: one queue for outbound email'),
        content:
          '## Context\n\nEvery service sends its own mail.\n\n'
          + '## Decision\n\nOne outbound queue, one template store.\n',
        decision_maker: alice.credentials.email,
      });
      [decision] = shortCodes(created);
      expect(decision, 'the ADR was created').toBeTruthy();
      ledger.add({
        kind: 'adr',
        label: decision,
        delete: async () => { await api.delete(`/api/adrs/${decision}`); },
      });
      const shown = await mcp.call('get_item', { short_code: decision });
      expect(shown).toContain('Draft');
      return { adr: decision, column: 'Draft', decision_maker: alice.credentials.email };
    });

    await step(alice, 'tries to call it decided without discussing it, and is told what she may do instead', async () => {
      const cli = await alice.cli();
      // The ADR board's graph is Draft → Discussion → Decided →
      // Superseded. Skipping the argument is exactly the move a hurried
      // team makes, and the board is right to refuse it.
      const refused = await cli.run(['adrs', 'transition', decision, '--to', columnId('Decided')]);
      expect(refused.code).toBe(1);
      const said = refused.stderr + refused.stdout;
      expect(said).toContain('invalid transition (422)');
      expect(said).toContain('Allowed target columns:');
      expect(said).toContain('Discussion');
      await cli.ok(['adrs', 'transition', decision, '--to', columnId('Discussion')]);
      const decided = await cli.json(['adrs', 'transition', decision, '--to', columnId('Decided')]);
      expect(decided.short_code).toBe(decision);
      return {
        refused_jump: 'Draft → Decided',
        offered: 'Discussion',
        walked: 'Draft → Discussion → Decided',
      };
    });

    await step(alice, 'hangs the decision off the initiative it was made for', async () => {
      // `informs` is org-admin only (an ADR speaking to a piece of work
      // is an editorial claim), which is why alice does this and not bob.
      const mcp = await alice.mcp();
      const linked = await mcp.call('link_items', {
        source: decision,
        target: initiative,
        relationship: 'informs',
      });
      expect(linked).toContain('informs');
      ledger.add({
        kind: 'relationship',
        label: `${decision} -[informs]-> ${initiative}`,
        delete: async () => {
          await mcp.call('unlink_items', {
            source: decision,
            target: initiative,
            relationship: 'informs',
          });
        },
      });
      return { adr: decision, informs: initiative };
    });

    await step(alice, 'writes the note that says how the decision gets implemented', async () => {
      const cli = await alice.cli();
      const doc = await cli.json([
        'documents', 'create',
        '--title', named('note: rolling out the outbound queue'),
        '--parent', initiative,
        '--content', '# Rollout\n\nOne service at a time, starting with sign-up mail.\n',
      ]);
      note = doc.short_code;
      ledger.add({
        kind: 'document',
        label: note,
        delete: async () => { await api.delete(`/api/documents/${note}`); },
      });
      return { document: note, supports: initiative, lifecycle: doc.lifecycle ?? 'draft' };
    });

    await step(alice, 'publishes the note — and finds the decision itself has no such switch', async () => {
      const page = await alice.gui();
      await openItem(page, note);
      await expect(page.locator('.kairos-lifecycle-badge')).toContainText('lifecycle: draft');
      const lifecycle = panel(page, 'Lifecycle');
      await lifecycle.locator('select').selectOption({ label: 'published' });
      await lifecycle.getByRole('button', { name: 'Set', exact: true }).click();
      await expect(page.locator('.kairos-lifecycle-badge')).toContainText('lifecycle: published', {
        timeout: 15_000,
      });
      // The two axes are easy to confuse and the product keeps them apart:
      // a DOCUMENT has an editorial lifecycle; an ADR's state is the board
      // column it sits in, and it is offered no lifecycle control at all.
      await openItem(page, decision);
      await expect(panel(page, 'Lifecycle')).toHaveCount(0);
      await expect(page.locator('.kairos-lifecycle-badge')).toHaveCount(0);
      return {
        document: note,
        document_lifecycle: 'published',
        adr: decision,
        adr_has_lifecycle_control: false,
        adr_state_is: 'the board column',
      };
    });

    await step(alice, 'months later, records the decision that replaces it', async () => {
      const mcp = await alice.mcp();
      const created = await mcp.call('create_item', {
        item_type: 'adr',
        board: 'adrs',
        title: named('decision: the provider owns the outbound queue'),
        content:
          '## Context\n\nRunning our own queue cost more than it saved.\n\n'
          + '## Decision\n\nHand outbound mail to the provider.\n',
        decision_maker: alice.credentials.email,
      });
      [replacement] = shortCodes(created);
      expect(replacement).toBeTruthy();
      ledger.add({
        kind: 'adr',
        label: replacement,
        delete: async () => { await api.delete(`/api/adrs/${replacement}`); },
      });
      const cli = await alice.cli();
      await cli.ok(['adrs', 'transition', replacement, '--to', columnId('Discussion')]);
      await cli.ok(['adrs', 'transition', replacement, '--to', columnId('Decided')]);
      return { adr: replacement, column: 'Decided', replaces: decision };
    });

    await step(alice, 'records that the new decision supersedes the old one, and retires the old one', async () => {
      const mcp = await alice.mcp();
      const linked = await mcp.call('link_items', {
        source: replacement,
        target: decision,
        relationship: 'supersedes',
      });
      expect(linked).toContain('supersedes');
      ledger.add({
        kind: 'relationship',
        label: `${replacement} -[supersedes]-> ${decision}`,
        delete: async () => {
          await mcp.call('unlink_items', {
            source: replacement,
            target: decision,
            relationship: 'supersedes',
          });
        },
      });
      const cli = await alice.cli();
      await cli.ok(['adrs', 'transition', decision, '--to', columnId('Superseded')]);
      // The board and the graph have to say the same thing: the edge is
      // the claim, the column is where a person looks for it.
      const listed = await cli.json(['adrs', 'list', '--limit', '100']);
      const rows: any[] = listed.items ?? listed;
      // The ADR DTO carries `column_id`, not a column name — resolve it
      // against the board the journey already read.
      const state = (code: string) => {
        const id = rows.find((a) => a.short_code === code)?.column_id;
        return adrBoard.columns.find((c: any) => c.id === id)?.name;
      };
      expect(state(decision)).toBe('Superseded');
      expect(state(replacement)).toBe('Decided');
      return {
        edge: `${replacement} -[supersedes]-> ${decision}`,
        [decision]: 'Superseded',
        [replacement]: 'Decided',
      };
    });

    await step(bob, 'reads both decisions and can get from either one to the other', async () => {
      const page = await bob.gui();
      await openBoard(page, 'adrs');
      await expect(
        adrColumn(page, 'Superseded').locator('article.kairos-card', { hasText: decision }),
      ).toBeVisible();
      await expect(
        adrColumn(page, 'Decided').locator('article.kairos-card', { hasText: replacement }),
      ).toBeVisible();

      await openItem(page, decision);
      await expect(
        relationshipGroup(page, 'superseded by').getByRole('link', { name: new RegExp(`^${replacement} — `) }),
      ).toBeVisible({ timeout: 15_000 });
      await openItem(page, replacement);
      await expect(
        relationshipGroup(page, 'supersedes').getByRole('link', { name: new RegExp(`^${decision} — `) }),
      ).toBeVisible({ timeout: 15_000 });

      // ADRs are never canvas nodes (the canvas is strategy/initiative/
      // task); the supersession shows up as supporting material instead.
      await panel(page, 'Relationships').getByRole('link', { name: 'Open the graph explorer' }).click();
      await page.waitForURL(new RegExp(`/search/relationships/${replacement}`));
      const material = panel(page, 'Supporting material');
      await expect(material).toBeVisible({ timeout: 20_000 });
      await expect(material.getByText(`${replacement} → supersedes`)).toBeVisible();
      return {
        old: `${decision} (Superseded) — superseded by ${replacement}`,
        new: `${replacement} (Decided) — supersedes ${decision}`,
        both_still_readable: true,
      };
    });
  },
);
