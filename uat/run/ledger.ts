// Everything a journey creates is recorded here so teardown can undo it in
// reverse order — the only way "self-cleaning against a deployment" holds.
// Deletion failures are collected and reported, never thrown: by the time
// teardown runs the journey has already passed or failed on its own steps.
//
// Reverse insertion order is the dependency order as long as journeys
// create parents before children, which they do naturally:
//   tenant → team → repository → forge connection → service account → key
//   strategy → initiative → task → relationship
export interface LedgerEntry {
  kind: string;
  label: string;
  delete: () => Promise<void>;
}

export interface TeardownFailure {
  kind: string;
  label: string;
  error: string;
}

export class Ledger {
  private readonly entries: LedgerEntry[] = [];
  readonly failures: TeardownFailure[] = [];

  add(entry: LedgerEntry): void {
    this.entries.push(entry);
  }

  /** Delete everything, last created first. Returns the failures. */
  async teardown(): Promise<TeardownFailure[]> {
    while (this.entries.length) {
      const entry = this.entries.pop()!;
      try {
        await entry.delete();
      } catch (err: any) {
        this.failures.push({ kind: entry.kind, label: entry.label, error: String(err?.message ?? err) });
      }
    }
    return this.failures;
  }

  get size(): number {
    return this.entries.length;
  }
}
