// An empty code index, for an upload (KAIROS-T-0744).
//
// `PUT /api/repositories/{slug}/code-indexes/{commit}` takes the SQLite
// file of `kairos index` (crates/kairos-index/src/schema.rs). The helper
// reads the schema and its version from that file, so the fixture follows
// the code, and makes the file with `node:sqlite` (Node 22.13 or later).

import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { DatabaseSync } from 'node:sqlite';

const SCHEMA_RS = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  '../../crates/kairos-index/src/schema.rs',
);

/** The bytes of a code index with each table and no row. */
export function emptyIndex(): Buffer {
  const source = fs.readFileSync(SCHEMA_RS, 'utf8');
  const version = /pub const SCHEMA_VERSION: i64 = (\d+);/.exec(source)?.[1];
  const schema = /const SCHEMA: &str = r#"([\s\S]*?)"#;/.exec(source)?.[1];
  if (!version || !schema) throw new Error(`no schema in ${SCHEMA_RS}`);
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'kairos-e2e-index-'));
  const file = path.join(dir, 'index.db');
  try {
    const db = new DatabaseSync(file);
    db.exec(schema);
    db.exec(`PRAGMA user_version = ${version}`);
    db.close();
    return fs.readFileSync(file);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

/** A commit id of 40 hex digits, from a number. */
export function commitId(n: number): string {
  return n.toString(16).padStart(40, '0');
}
