// What the journeys actually exercised (KAIROS-I-0013 D1).
//
// The drift gate measures RUNTIME, not source text: `McpSession` records
// every tool it calls and `Cli` every noun it runs, wherever the call was
// made from — a fixture, a helper, a journey. A regex over the journey
// files would pass on commented-out code and miss anything reached
// indirectly, and the question we care about is "did a persona do this",
// which only the run can answer.
//
// The records go to a file, not a module-level Set: Playwright starts a
// fresh worker process for a dependent project (the gate is one) and
// after any test failure, so in-memory state would silently lose whole
// journeys and the gate would report gaps that are not real. One
// append-only JSONL per run, in that run's report directory.
import fs from 'node:fs';
import path from 'node:path';
import { runContext } from './context';

export type SurfaceKind = 'mcp' | 'cli';

function ledgerPath(): string {
  return path.join(runContext().reportDir, 'surfaces.jsonl');
}

function append(record: Record<string, string>): void {
  try {
    const file = ledgerPath();
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.appendFileSync(file, `${JSON.stringify(record)}\n`);
  } catch {
    // Coverage bookkeeping must never fail a journey; a missing ledger
    // surfaces as an honest gap in the gate instead.
  }
}

/** One MCP tool call or CLI noun invocation, as it happens. */
export function recordSurface(kind: SurfaceKind, name: string): void {
  if (name) append({ kind, name });
}

/** A journey that started (the gate needs to know the run was complete). */
export function recordJourney(id: string): void {
  if (id) append({ kind: 'journey', name: id });
}

export interface SurfaceUsage {
  mcp: string[];
  cli: string[];
  journeys: string[];
}

/** Everything recorded so far, sorted — for the gate and the report. */
export function surfaceUsage(): SurfaceUsage {
  const buckets: Record<string, Set<string>> = {
    mcp: new Set(),
    cli: new Set(),
    journey: new Set(),
  };
  let raw = '';
  try {
    raw = fs.readFileSync(ledgerPath(), 'utf8');
  } catch {
    raw = '';
  }
  for (const line of raw.split('\n')) {
    if (!line.trim()) continue;
    try {
      const { kind, name } = JSON.parse(line) as { kind: string; name: string };
      buckets[kind]?.add(name);
    } catch {
      // A torn final line (killed run) is not worth failing over.
    }
  }
  return {
    mcp: [...buckets.mcp].sort(),
    cli: [...buckets.cli].sort(),
    journeys: [...buckets.journey].sort(),
  };
}
