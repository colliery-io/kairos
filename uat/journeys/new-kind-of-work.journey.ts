// J11 — "A team makes support requests first-class" (KAIROS-I-0014).
//
// Templates and metadata definitions have admin screens and a create-from
// path (A-0003), and no journey uses them the way an admin does: define a
// field, put it on a template, let the template stamp it, then try to take
// the field away again.
//
// The last part is the point. Retiring a field looks harmless until you
// ask what happens to the values already stamped on other people's work.
// Kairos answers by refusing: a definition cannot be deleted while
// anything still references it, and it says how many things do. So the
// admin's real move is to take the field OFF the template — new work stops
// collecting it, and the work that already carries it keeps its value.
import { expect, type Locator, type Page } from '@playwright/test';
import { named } from '../run/context';
import { journey, step } from '../run/narrate';
import { openItem, panel } from '../surfaces/gui';
import { shortCodes } from '../surfaces/mcp';

/** A form control by its visible label, inside a panel. */
function field(scope: Locator, page: Page, label: string): Locator {
  return scope.locator('.cl-field', { has: page.getByText(label, { exact: true }) }).first();
}

journey(
  'new-kind-of-work',
  'A team makes support requests first-class',
  { humans: ['alice', 'bob'] },
  async ({ cast, ledger }) => {
    const alice = cast.human('alice');
    const bob = cast.human('bob');
    const api = await alice.api();
    const stamp = Date.now().toString(36);
    const fieldSlug = `uat-severity-${stamp}`;
    const templateSlug = `uat-support-${stamp}`;
    const starter = '## What happened\n\n## Who is affected\n\n## What we did\n';
    let definitionId = '';
    let templateId = '';
    let request = '';
    let writeUp = '';

    await step(alice, 'defines the field every support request will need', async () => {
      const page = await alice.gui();
      await page.goto('/admin/metadata');
      const form = panel(page, 'Create definition');
      await field(form, page, 'Name').locator('input').fill('UAT Severity');
      await field(form, page, 'Slug').locator('input').fill(fieldSlug);
      await field(form, page, 'Type').locator('select').selectOption('enum');
      for (const option of ['sev1', 'sev2', 'sev3']) {
        await form.getByPlaceholder('New option').fill(option);
        await form.getByRole('button', { name: 'Add option', exact: true }).click();
      }
      await form.getByRole('button', { name: 'Create definition', exact: true }).click();
      await expect(page.getByText(fieldSlug).first()).toBeVisible({ timeout: 15_000 });
      const defs = await api.get('/api/metadata-definitions');
      const mine = (defs.items ?? defs).find((d: any) => d.slug === fieldSlug);
      expect(mine, 'the definition exists').toBeTruthy();
      definitionId = mine.id;
      ledger.add({
        kind: 'metadata-definition',
        label: fieldSlug,
        delete: async () => { await api.delete(`/api/metadata-definitions/${definitionId}`); },
      });
      return { field: fieldSlug, type: mine.field_type ?? mine.type, values: 'sev1 | sev2 | sev3' };
    });

    await step(alice, 'turns the recurring write-up into a template that stamps that field', async () => {
      const page = await alice.gui();
      await page.goto('/admin/templates');
      const form = panel(page, 'Create template');
      await field(form, page, 'Name').locator('input').fill('UAT Support request');
      await field(form, page, 'Slug').locator('input').fill(templateSlug);
      await field(form, page, 'Starter content (markdown)').locator('textarea').fill(starter);
      await form.getByRole('button', { name: 'Add metadata field', exact: true }).click();
      await field(form, page, 'Definition').locator('select').selectOption(fieldSlug);
      // Only an association carrying a DEFAULT gets stamped at
      // create-from-template time — a required field with no default just
      // tells the author it is expected.
      await field(form, page, 'Default (optional)').locator('input').fill('sev3');
      await form.getByRole('button', { name: 'Create template', exact: true }).click();
      await expect(page.getByText(templateSlug).first()).toBeVisible({ timeout: 15_000 });
      const templates = await api.get('/api/templates?limit=100');
      const mine = (templates.items ?? templates).find((t: any) => t.slug === templateSlug);
      expect(mine, 'the template exists').toBeTruthy();
      templateId = mine.id;
      ledger.add({
        kind: 'template',
        label: templateSlug,
        delete: async () => { await api.delete(`/api/templates/${templateId}`); },
      });
      const detail = await api.get(`/api/templates/${templateId}`);
      const stamped = (detail.metadata ?? []).map((m: any) => `${m.slug}=${m.default_value}`);
      expect(stamped).toContain(`${fieldSlug}=sev3`);
      return { template: templateSlug, stamps: stamped.join(', '), starter_sections: 3 };
    });

    await step(bob, 'raises the new kind of work: a support request on his own board', async () => {
      const mcp = await bob.mcp();
      const me = await (await bob.api()).whoami();
      const team = (me.teams ?? [])[0]?.slug as string;
      expect(team, 'bob is on a team').toBeTruthy();
      const created = await mcp.call('create_item', {
        item_type: 'task',
        board: `${team}-delivery`,
        task_type: 'support',
        title: named('support: invoices stopped arriving for one tenant'),
        content: 'Reported by the customer at 09:12.',
      });
      [request] = shortCodes(created);
      expect(request).toBeTruthy();
      ledger.add({
        kind: 'task',
        label: request,
        delete: async () => { await api.delete(`/api/tasks/${request}`); },
      });
      const shown = await mcp.call('get_item', { short_code: request });
      // KAIROS-T-0077: a support-type task lands in the Support lane
      // without anyone asking for it.
      expect(shown).toContain('support');
      return { request, board: `${team}-delivery`, task_type: 'support', lane: 'support' };
    });

    await step(bob, 'opens the intake write-up from the template — and finds it already filled in', async () => {
      const mcp = await bob.mcp();
      const created = await mcp.call('create_item', {
        item_type: 'document',
        parent: request,
        template: templateSlug,
        title: named('intake: invoices stopped arriving'),
      });
      [writeUp] = shortCodes(created).filter((code) => code !== request);
      expect(writeUp, 'the document was created').toBeTruthy();
      ledger.add({
        kind: 'document',
        label: writeUp,
        delete: async () => { await api.delete(`/api/documents/${writeUp}`); },
      });
      ledger.add({
        kind: 'metadata-value',
        label: `${fieldSlug} on ${writeUp}`,
        delete: async () => {
          // Cleared before the document goes: a definition cannot be
          // deleted while a value still references it, and a soft-deleted
          // item keeps its metadata rows.
          await mcp.call('set_metadata', { short_code: writeUp, values: { [fieldSlug]: null } });
        },
      });
      const shown = await mcp.call('get_item', { short_code: writeUp });
      expect(shown, 'the template seeded the content').toContain('What happened');
      expect(shown, 'the template stamped the field').toContain(`${fieldSlug}: sev3`);
      return {
        document: writeUp,
        supports: request,
        from_template: templateSlug,
        stamped: `${fieldSlug}=sev3`,
        content_seeded: true,
      };
    });

    await step(bob, 'works it: this one is worse than the default says', async () => {
      const mcp = await bob.mcp();
      await mcp.call('set_metadata', { short_code: writeUp, values: { [fieldSlug]: 'sev1' } });
      const shown = await mcp.call('get_item', { short_code: writeUp });
      expect(shown).toContain(`${fieldSlug}: sev1`);
      const page = await bob.gui();
      await openItem(page, writeUp);
      // The Metadata panel labels a field by its definition NAME, not its
      // slug, and an enum renders as a select holding the current value.
      const row = panel(page, 'Metadata').locator('.kairos-metadata__field', {
        hasText: 'UAT Severity',
      });
      await expect(row).toBeVisible({ timeout: 15_000 });
      await expect(row.locator('select')).toHaveValue('sev1');
      // The whole reason to make the field first-class: the team can ask
      // "what is on fire?" and get an answer.
      const cli = await bob.cli();
      const hits = await cli.json(['search', '--metadata', `${fieldSlug}=sev1`, '--limit', '50']);
      const codes = [
        ...(hits.results?.documents ?? []),
        ...(hits.results?.tasks ?? []),
      ].map((d: any) => d.short_code);
      expect(codes).toContain(writeUp);
      return { document: writeUp, severity: 'sev3 → sev1', found_by: `${fieldSlug}=sev1`, hits: codes.length };
    });

    await step(alice, 'decides the field was a mistake and tries to retire it', async () => {
      const page = await alice.gui();
      await page.goto('/admin/metadata');
      const row = page.locator('.cl-stack', { has: page.getByText(fieldSlug, { exact: true }) }).last();
      await row.getByRole('button', { name: 'Delete', exact: true }).click();
      // Refused, and the refusal is the feature: a field cannot be retired
      // out from under the work that already carries it.
      const alert = page.locator('.cl-alert[role="alert"]');
      await expect(alert).toBeVisible({ timeout: 15_000 });
      await expect(alert).toContainText('in use');
      await expect(alert).toContainText('DEFINITION_IN_USE');
      const still = await api.raw('DELETE', `/api/metadata-definitions/${definitionId}`);
      expect(still.status).toBe(409);
      return {
        field: fieldSlug,
        refused: 'DEFINITION_IN_USE',
        item_values: still.body.error.details.item_values,
        template_fields: still.body.error.details.template_fields,
      };
    });

    await step(alice, 'takes the field off the template instead, so new requests stop collecting it', async () => {
      const page = await alice.gui();
      await page.goto('/admin/templates');
      const row = page.locator('.cl-stack', { has: page.getByText(templateSlug, { exact: true }) }).last();
      await row.getByRole('button', { name: 'Edit', exact: true }).click();
      await expect(row.getByRole('button', { name: 'Remove field', exact: true })).toBeVisible({
        timeout: 15_000,
      });
      await row.getByRole('button', { name: 'Remove field', exact: true }).click();
      await row.getByRole('button', { name: 'Save template', exact: true }).click();
      await expect(page.getByText(`Template "UAT Support request" updated.`)).toBeVisible({
        timeout: 15_000,
      });
      const detail = await api.get(`/api/templates/${templateId}`);
      expect(detail.metadata ?? []).toHaveLength(0);
      return { template: templateSlug, fields_now: 0, new_requests_stamp: 'nothing' };
    });

    await step(bob, 'reads his write-up again: the value he set is still there', async () => {
      // The bit people get wrong. Taking a field off a template changes
      // what the NEXT item gets; it does not reach backwards.
      const after = await api.raw('DELETE', `/api/metadata-definitions/${definitionId}`);
      expect(after.status, 'the definition is still pinned by the stamped value').toBe(409);
      expect(after.body.error.details.template_fields).toBe(0);
      expect(after.body.error.details.item_values).toBeGreaterThan(0);
      const mcp = await bob.mcp();
      const shown = await mcp.call('get_item', { short_code: writeUp });
      expect(shown).toContain(`${fieldSlug}: sev1`);
      const cli = await bob.cli();
      const hits = await cli.json(['search', '--metadata', `${fieldSlug}=sev1`, '--limit', '50']);
      const codes = (hits.results?.documents ?? []).map((d: any) => d.short_code);
      expect(codes).toContain(writeUp);
      return {
        document: writeUp,
        still_reads: `${fieldSlug}=sev1`,
        still_findable_by_it: true,
        definition_still_pinned_by: `${after.body.error.details.item_values} item value(s)`,
      };
    });
  },
);
