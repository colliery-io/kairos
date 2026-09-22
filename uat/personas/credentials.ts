// Who the personas are on this target. Compose defaults are the seed's
// Dex users; a deployment overrides through UAT_PERSONA_<NAME>_EMAIL /
// _PASSWORD. `agent` has no password: it is a service account whose API
// key a journey mints (or UAT_PERSONA_AGENT_API_KEY supplies).
export type Human = 'alice' | 'bob' | 'carol' | 'newhire';
export type PersonaName = Human | 'agent';

export interface Credentials {
  email: string;
  password: string;
}

const DEFAULTS: Record<Human, Credentials | null> = {
  alice: { email: 'alice@kairos.test', password: 'alice-password' },
  bob: { email: 'bob@kairos.test', password: 'bob-password' },
  carol: { email: 'carol@kairos.test', password: 'carol-password' },
  // No seeded fourth human: journeys substitute bob and say so.
  newhire: null,
};

export const ROLES: Record<PersonaName, string> = {
  alice: 'org admin',
  bob: 'platform engineer (team member)',
  carol: 'web engineer (other team)',
  newhire: 'newly added member',
  agent: 'coding agent (service account)',
};

/** Credentials for a human persona, or null when none are configured. */
export function credentialsFor(name: Human): Credentials | null {
  const key = name.toUpperCase();
  const email = process.env[`UAT_PERSONA_${key}_EMAIL`];
  const password = process.env[`UAT_PERSONA_${key}_PASSWORD`];
  if (email && password) return { email, password };
  return DEFAULTS[name];
}
