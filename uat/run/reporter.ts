// Renders the run report: reports/<run>/report.md (for people) and
// report.json (for machines) from the journey records the narrator
// attaches to each test. A journey that died before attaching (a beforeAll
// throw, a timeout in setup) still gets a row from the test result itself.
import type { FullConfig, FullResult, Reporter, TestCase, TestResult } from '@playwright/test/reporter';
import fs from 'node:fs';
import path from 'node:path';
import { runContext } from './context';
import { ATTACHMENT, type JourneyRecord, type StepRecord } from './narrate';

interface JourneyResult {
  record: JourneyRecord;
  status: TestResult['status'];
  durationMs: number;
  error?: string;
}

export const COVERAGE = 'uat-coverage';

interface CoverageResult {
  mcp?: { offered: number; exercised: number };
  cli?: { offered: number; exercised: number };
  allowed?: string[];
  uncovered?: string[];
  stale?: string[];
  skipped?: boolean;
  status: string;
}

const ICON: Record<string, string> = { passed: '✅', failed: '❌', skipped: '⏭️', timedOut: '❌', interrupted: '❌' };

function md(value: unknown): string {
  return String(value ?? '').replace(/\|/g, '\\|').replace(/\n/g, ' ');
}

function observedText(step: StepRecord): string {
  if (step.status === 'skipped') return `skipped: ${step.reason ?? ''}`;
  const parts: string[] = [];
  if (step.observed) {
    for (const [k, v] of Object.entries(step.observed)) {
      if (v === undefined) continue;
      parts.push(`${k}: ${Array.isArray(v) ? v.join(', ') : v}`);
    }
  }
  if (step.error) parts.push(`error: ${step.error}`);
  for (const shot of step.screenshots) parts.push(`screenshot: ./${path.basename(shot)}`);
  return parts.length ? parts.join('; ') : '—';
}

export default class UatReporter implements Reporter {
  private readonly journeys: JourneyResult[] = [];
  private coverage: CoverageResult | undefined;
  private startedAt = new Date();

  onBegin(_config: FullConfig): void {
    this.startedAt = new Date();
  }

  onTestEnd(test: TestCase, result: TestResult): void {
    // The coverage gate is not a journey; it renders as its own section.
    const coverage = result.attachments.find((a) => a.name === COVERAGE && a.body);
    if (coverage) {
      this.coverage = { ...JSON.parse(coverage.body!.toString('utf8')), status: result.status };
      return;
    }
    if (test.title.startsWith('every MCP tool')) {
      this.coverage = { skipped: result.status === 'skipped', status: result.status };
      return;
    }
    const attachment = result.attachments.find((a) => a.name === ATTACHMENT && a.body);
    const title = test.title.replace(/\s+@[\w-]+$/, '');
    const id = test.title.match(/@([\w-]+)$/)?.[1] ?? title;
    const record: JourneyRecord = attachment
      ? JSON.parse(attachment.body!.toString('utf8'))
      : { id, title, mode: runContext().mode, steps: [], teardownFailures: [], traces: [] };
    this.journeys.push({
      record,
      status: result.status,
      durationMs: result.duration,
      error: result.error?.message?.split('\n').slice(0, 4).join(' '),
    });
  }

  async onEnd(result: FullResult): Promise<void> {
    const ctx = runContext();
    fs.mkdirSync(ctx.reportDir, { recursive: true });
    const passed = this.journeys.filter((j) => j.status === 'passed').length;
    const failed = this.journeys.length - passed;
    const skippedSteps = this.journeys.reduce(
      (n, j) => n + j.record.steps.filter((s) => s.status === 'skipped').length,
      0,
    );
    const stamp = this.startedAt.toISOString().replace(/\.\d+Z$/, 'Z');

    const lines: string[] = [];
    lines.push(`# Kairos UAT — run ${stamp} (${ctx.run})`);
    lines.push('');
    lines.push(`Target: ${ctx.server} (${ctx.mode} mode, tenant \`${ctx.tenant}\`)`);
    lines.push('');
    lines.push(
      `**Result: ${this.journeys.length} journeys, ${passed} passed, ${failed} failed, ${skippedSteps} steps skipped.**`,
    );
    lines.push('');
    for (const j of this.journeys) {
      const secs = (j.durationMs / 1000).toFixed(1);
      lines.push(`## ${j.record.id} — ${j.record.title}   ${ICON[j.status] ?? j.status} ${secs}s`);
      lines.push('');
      if (j.record.steps.length === 0) {
        lines.push(`_No steps recorded${j.error ? `: ${md(j.error)}` : ''}._`);
        lines.push('');
        continue;
      }
      lines.push('| # | Persona | Step | Observed | Status |');
      lines.push('|---|---|---|---|---|');
      for (const s of j.record.steps) {
        lines.push(
          `| ${s.index} | ${md(s.persona)} | ${md(s.narration)} | ${md(observedText(s))} | ${ICON[s.status]} |`,
        );
      }
      lines.push('');
      if (j.error && !j.record.steps.some((s) => s.status === 'failed')) {
        lines.push(`Journey error outside a step: ${md(j.error)}`);
        lines.push('');
      }
      if (j.record.traces.length) {
        lines.push(`Traces: ${j.record.traces.map((t) => `./${path.basename(t)}`).join(', ')}`);
        lines.push('');
      }
      if (j.record.teardownFailures.length) {
        lines.push('Teardown left behind (clean up by hand):');
        for (const f of j.record.teardownFailures) lines.push(`- ${f.kind} ${f.label}: ${f.error}`);
        lines.push('');
      }
    }

    // Always present, so a filtered run cannot read as "no gate here".
    {
      const coverage = this.coverage;
      lines.push(`## Surface coverage   ${coverage ? (ICON[coverage.status] ?? coverage.status) : '⏭️'}`);
      lines.push('');
      if (!coverage || coverage.skipped || !coverage.mcp) {
        lines.push(
          '_Not measured: the gate needs a whole compose run. A filtered run, ' +
            'or a deployment run whose compose-only steps are skipped, cannot ' +
            'speak for the product._',
        );
      } else {
        lines.push(
          `MCP ${coverage.mcp.exercised}/${coverage.mcp.offered} tools, ` +
            `CLI ${coverage.cli!.exercised}/${coverage.cli!.offered} nouns, ` +
            `${(coverage.allowed ?? []).length} allow-listed.`,
        );
        for (const [heading, rows] of [
          ['Allow-listed (no journey touches these yet)', coverage.allowed ?? []],
          ['UNCOVERED — needs a step or a reason', coverage.uncovered ?? []],
          ['Stale allow-list entries — delete them', coverage.stale ?? []],
        ] as const) {
          if (!rows.length) continue;
          lines.push('');
          lines.push(`${heading}:`);
          for (const row of rows) lines.push(`- ${row}`);
        }
      }
      lines.push('');
    }

    const mdPath = path.join(ctx.reportDir, 'report.md');
    const jsonPath = path.join(ctx.reportDir, 'report.json');
    fs.writeFileSync(mdPath, lines.join('\n'));
    fs.writeFileSync(
      jsonPath,
      JSON.stringify(
        {
          run: ctx.run,
          startedAt: this.startedAt.toISOString(),
          target: ctx.server,
          mode: ctx.mode,
          tenant: ctx.tenant,
          result: result.status,
          coverage: this.coverage,
          journeys: this.journeys,
        },
        null,
        2,
      ),
    );
    console.log(
      `\nUAT ${result.status}: ${this.journeys.length} journeys, ${passed} passed, ${failed} failed, ${skippedSteps} steps skipped\nReport: ${mdPath}`,
    );
  }
}
