// J25 — "A team writes the vision of its product" (COLLIERY-T-0269,
// COLLIERY-I-0019).
//
// The vision of a repository is a document. It says why the repository
// exists. Until COLLIERY-T-0269 a document had to support a work item to
// have an owner, so a team hung its vision off some strategy that the
// vision was not about. The owner decided the model on 2026-09-29:
//
//   document -> board                        THE OWNER. The board gives the
//                                            right to edit.
//   document -> repository, by `impacts`     What the document is ABOUT. It
//                                            gives no right.
//
// The story walks the two links with the surfaces that people and agents
// use: an engineer makes the vision with the CLI, an agent that stands in
// the repository finds it with `get_repository`, an engineer of a
// different team reads it and cannot change it, and an admin gives it to a
// different board.
//
// It uses the seeded repositories: `payments-api` of the team platform,
// and `portal-web` of the team web. The one thing that it makes is the
// document, and the teardown archives it.
import { expect } from '@playwright/test';
import { named } from '../run/context';
import { journey, step } from '../run/narrate';
import { openItem, openTeam, panel } from '../surfaces/gui';
import { field } from '../surfaces/mcp';

const PRODUCT_REPO = process.env.UAT_PLATFORM_REPO ?? 'payments-api';
const OTHER_REPO = process.env.UAT_WEB_REPO ?? 'portal-web';
const OWNER_BOARD = process.env.UAT_PLATFORM_BOARD ?? 'platform-delivery';
const OTHER_BOARD = process.env.UAT_WEB_BOARD ?? 'web-delivery';

journey(
  'product-vision',
  'A team writes the vision of its product',
  { humans: ['alice', 'bob', 'carol'] },
  async ({ cast, ledger }) => {
    const alice = cast.human('alice');
    const bob = cast.human('bob');
    const carol = cast.human('carol');
    const api = await alice.api();
    const title = named('the vision of payments-api');
    let vision = '';

    await step(bob, 'looks for a template for the vision of a product, and finds one', async () => {
      const templates = (await (await bob.api()).get('/api/templates?limit=200')).items as any[];
      const template = templates.find((t) => t.name === 'Product Vision');
      expect(template, 'the tenant has the template Product Vision').toBeTruthy();
      const detail = await (await bob.api()).get(`/api/templates/${template.id}`);
      for (const heading of ['## Purpose', '## Who It Is For', '## What It Is Not']) {
        expect(detail.content).toContain(heading);
      }
      // "Company Vision" is a different template, and it stays.
      expect(templates.some((t) => t.name === 'Company Vision')).toBe(true);
      return { template: template.name, sections: 6 };
    });

    await step(bob, 'makes the vision with no work item above it, and his team board is its owner', async () => {
      const cli = await bob.cli();
      // A document must have an owner board: the CLI does not send a
      // create with no board (COLLIERY-T-3109).
      const refused = await cli.run(['documents', 'create', '--title', title]);
      expect(refused.code).not.toBe(0);
      expect(refused.stderr).toContain('--board');

      const templates = (await (await bob.api()).get('/api/templates?limit=200')).items as any[];
      const template = templates.find((t) => t.name === 'Product Vision');
      const created = await cli.json([
        'documents', 'create',
        '--title', title,
        '--board', OWNER_BOARD,
        '--template', template.id,
      ]);
      vision = created.short_code;
      ledger.add({
        kind: 'document',
        label: vision,
        delete: async () => { await api.delete(`/api/documents/${vision}`); },
      });
      const board = await api.boardBySlug(OWNER_BOARD);
      expect(created.board_id).toBe(board.id);
      expect(created.impacts).toEqual([]);
      const relationships = await api.get(`/api/documents/${vision}/relationships`);
      expect(relationships.incoming).toEqual([]);
      return { document: vision, owner_board: OWNER_BOARD, supports: 'nothing' };
    });

    await step(bob, 'says which repository the vision is about', async () => {
      const cli = await bob.cli();
      const said = await cli.ok(['repos', 'link', vision, PRODUCT_REPO]);
      expect(said).toContain(`${vision} impacts the repository ${PRODUCT_REPO}.`);
      // A task does not impact a repository: it links to one.
      const tasks = (await api.get('/api/tasks?limit=1')).items as any[];
      const refused = await cli.run(['repos', 'link', tasks[0].short_code, PRODUCT_REPO]);
      expect(refused.code).not.toBe(0);
      expect(refused.stderr + refused.stdout).toContain(
        'Only a document or an ADR can impact a repository.',
      );
      const shown = await cli.ok(['repos', 'get', PRODUCT_REPO]);
      expect(shown).toContain('Documents and ADRs that impact this repository:');
      expect(shown).toContain(vision);
      expect(shown).toContain('document (vision)');
      return { document: vision, impacts: PRODUCT_REPO };
    });

    await step(bob, 'an agent in the repository reads what the repository is for before it plans work', async () => {
      const mcp = await bob.mcp();
      const repository = await mcp.call('get_repository', { repository: PRODUCT_REPO });
      expect(repository).toContain('## Documents and ADRs that impact this repository');
      expect(repository).toContain(`- ${vision} — ${title} · document (vision) · lifecycle: draft`);
      const item = await mcp.call('get_item', { short_code: vision });
      expect(field(item, '- owner board')).toBe(OWNER_BOARD);
      expect(field(item, '- impacts')).toContain(`repository ${PRODUCT_REPO}`);
      expect(item).toContain('## Purpose');
      // The document is not a card of its owner board.
      const board = await mcp.call('board_items', { board: OWNER_BOARD });
      expect(board).not.toContain(vision);
      return { reads: vision, owner_board: OWNER_BOARD, on_the_board: false };
    });

    await step(bob, 'says that the vision is about the portal too, which a different team owns', async () => {
      const mcp = await bob.mcp();
      // No right on the repository is necessary.
      const linked = await mcp.call('link_items', {
        source: vision,
        target: OTHER_REPO,
        relationship: 'impacts',
      });
      expect(linked).toBe(`Linked ${vision} -[impacts]-> repository ${OTHER_REPO}.`);
      const again = await mcp.refused('link_items', {
        source: vision,
        target: OTHER_REPO,
        relationship: 'impacts',
      });
      expect(again).toContain('ALREADY_LINKED');
      return { document: vision, impacts: [PRODUCT_REPO, OTHER_REPO] };
    });

    await step(carol, 'finds the vision from her repository, reads it, and cannot change it', async () => {
      const mcp = await carol.mcp();
      const found = await mcp.call('search', { filter: { repository: OTHER_REPO, entity_type: ['document'] } });
      expect(found).toContain(vision);
      const cli = await carol.cli();
      const listed = await cli.json(['documents', 'list', '--repo', OTHER_REPO]);
      expect((listed.items as any[]).map((d) => d.short_code)).toContain(vision);
      // The link gives no right: carol is a member of the team that owns
      // the portal, and the owner of the vision is the board of platform.
      const refused = await mcp.refused('edit_item', {
        short_code: vision,
        search: '## Purpose',
        replace: '## Purpose\n\nThe portal decides.',
      });
      expect(refused).toContain('FORBIDDEN');
      expect(refused).toContain('manage_documents');
      const unlink = await mcp.refused('unlink_items', {
        source: vision,
        target: OTHER_REPO,
        relationship: 'impacts',
      });
      expect(unlink).toContain('FORBIDDEN');
      expect(unlink).toContain('You need no right on the repository.');
      return { who: carol.credentials.email, reads: vision, edits: false };
    });

    await step(alice, 'opens the vision and sees its owner and what it is about', async () => {
      const page = await alice.gui();
      await openItem(page, vision);
      const board = await api.boardBySlug(OWNER_BOARD);
      await expect(panel(page, 'Owner board')).toContainText(board.name);
      await expect(panel(page, 'Owner board')).toContainText(
        'The document is not a card on the board.',
      );
      await expect(panel(page, 'Impacts')).toContainText(PRODUCT_REPO);
      await expect(panel(page, 'Impacts')).toContainText(OTHER_REPO);
      await openTeam(page, 'platform');
      const row = panel(page, 'Repositories').locator(`[data-repo="${PRODUCT_REPO}"]`);
      await row.getByRole('button', { name: 'Show documents' }).click();
      await expect(row.getByRole('link', { name: new RegExp(`^${vision} — `) })).toBeVisible({
        timeout: 15_000,
      });
      return { document: vision, owner_board: board.name, shown_for: PRODUCT_REPO };
    });

    await step(bob, 'tries to give the vision to the board of a different team, and is told what he needs', async () => {
      const mcp = await bob.mcp();
      const refused = await mcp.refused('move_item', { short_code: vision, to_board: OTHER_BOARD });
      expect(refused).toContain('FORBIDDEN');
      expect(refused).toContain(`You do not have it on the board "${OTHER_BOARD}", the new board.`);
      // And the owner board of the vision cannot go.
      const last = await mcp.refused('move_item', { short_code: vision });
      // COLLIERY-T-3109: the owner board of a document cannot be removed.
      expect(last).toContain('to_board');
      expect(last).toContain('you cannot remove it');
      return { refused_move_to: OTHER_BOARD, needs: 'manage_documents on the two boards' };
    });

    await step(alice, 'gives the vision to the board of the team web, and that team can change it', async () => {
      const cli = await alice.cli();
      const moved = await cli.ok(['documents', 'move', vision, '--to-board', OTHER_BOARD]);
      expect(moved).toContain(`Kairos moved the document ${vision} to the owner board`);
      const mcp = await carol.mcp();
      const edited = await mcp.call('edit_item', {
        short_code: vision,
        search: '## Purpose',
        replace: '## Purpose\n\nOne place to pay.',
      });
      expect(edited).toContain(`Edited ${vision}`);
      // bob made the document, so he can edit it as before.
      const item = await (await bob.mcp()).call('get_item', { short_code: vision });
      expect(field(item, '- owner board')).toBe(OTHER_BOARD);
      expect(item).toContain('One place to pay.');
      return { document: vision, owner_board: OTHER_BOARD, edited_by: carol.credentials.email };
    });

    await step(carol, 'removes the link to the portal, because the vision is about the payments service', async () => {
      const cli = await carol.cli();
      const said = await cli.ok(['repos', 'unlink', vision, OTHER_REPO]);
      expect(said).toContain(`${vision} does not impact the repository ${OTHER_REPO}.`);
      const document = await api.get(`/api/documents/${vision}`);
      expect((document.impacts as any[]).map((i) => i.repository.slug)).toEqual([PRODUCT_REPO]);
      const activity = (await api.get(`/api/activity?entity_id=${document.id}&limit=50`)).items as any[];
      const actions = activity.map((entry) => entry.action);
      expect(actions).toContain('relationship_add');
      expect(actions).toContain('relationship_remove');
      expect(actions).toContain('update');
      return { document: vision, impacts: [PRODUCT_REPO], activity: actions.length };
    });

    await step(alice, 'gives the vision back to the team that owns the product', async () => {
      const mcp = await alice.mcp();
      const moved = await mcp.call('move_item', { short_code: vision, to_board: OWNER_BOARD });
      expect(moved).toBe(`Moved ${vision}: owner board ${OTHER_BOARD} -> ${OWNER_BOARD}.`);
      const again = await mcp.call('move_item', { short_code: vision, to_board: OWNER_BOARD });
      expect(again).toBe(`No change to ${vision}: its owner board is ${OWNER_BOARD} already.`);
      return { document: vision, owner_board: OWNER_BOARD };
    });
  },
);
