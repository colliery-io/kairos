-- KAIROS-T-0339 (COLLIERY-I-0611): the provider of the code index
-- summaries and the provider of the code index vectors of a tenant, with
-- the secrets sealed with KAIROS_SECRETS_KEY (as repository_credentials
-- are, COLLIERY-T-3105).
--
--   code_index_settings   One row at most (id = 1). A tenant with no row
--                         uses the embedded model for both.
--
-- summary_provider: 'embedded' (the model in the server), 'ollama-cloud'
-- (an OpenAI-compatible chat endpoint: base URL, model, API key) or
-- 'bedrock' (AWS Bedrock: region, model id, credentials). vector_provider:
-- 'embedded' or 'remote' (an OpenAI-compatible embeddings endpoint).
-- *_ciphertext, *_nonce, *_key_id: the sealed secret, the three together or
-- none. concurrency: the requests that a hosted summarizer sends at a time.
--
-- WHAT THIS DOES TO THE DATA OF A TENANT
--
-- 1 new table, empty. No row of a table that the tenant has is changed or
-- deleted.
--
-- Re-runnable: the statement has a guard, and the second run changes
-- nothing.
CREATE TABLE IF NOT EXISTS code_index_settings (
    id                     INTEGER PRIMARY KEY CHECK (id = 1),
    summary_provider       TEXT NOT NULL DEFAULT 'embedded'
                           CHECK (summary_provider IN ('embedded', 'ollama-cloud', 'bedrock')),
    summary_base_url       TEXT,
    summary_model          TEXT,
    summary_region         TEXT,
    summary_ciphertext     BYTEA CHECK (summary_ciphertext IS NULL OR octet_length(summary_ciphertext) > 16),
    summary_nonce          BYTEA CHECK (summary_nonce IS NULL OR octet_length(summary_nonce) = 12),
    summary_key_id         TEXT,
    summary_secret_set_by  UUID,
    summary_secret_set_at  TIMESTAMPTZ,
    vector_provider        TEXT NOT NULL DEFAULT 'embedded'
                           CHECK (vector_provider IN ('embedded', 'remote')),
    vector_base_url        TEXT,
    vector_model           TEXT,
    vector_ciphertext      BYTEA CHECK (vector_ciphertext IS NULL OR octet_length(vector_ciphertext) > 16),
    vector_nonce           BYTEA CHECK (vector_nonce IS NULL OR octet_length(vector_nonce) = 12),
    vector_key_id          TEXT,
    vector_secret_set_by   UUID,
    vector_secret_set_at   TIMESTAMPTZ,
    concurrency            INTEGER NOT NULL DEFAULT 4 CHECK (concurrency BETWEEN 1 AND 32),
    updated_by             UUID NOT NULL,
    updated_at             TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK ((summary_ciphertext IS NULL) = (summary_nonce IS NULL)
       AND (summary_ciphertext IS NULL) = (summary_key_id IS NULL)
       AND (summary_ciphertext IS NULL) = (summary_secret_set_by IS NULL)
       AND (summary_ciphertext IS NULL) = (summary_secret_set_at IS NULL)),
    CHECK ((vector_ciphertext IS NULL) = (vector_nonce IS NULL)
       AND (vector_ciphertext IS NULL) = (vector_key_id IS NULL)
       AND (vector_ciphertext IS NULL) = (vector_secret_set_by IS NULL)
       AND (vector_ciphertext IS NULL) = (vector_secret_set_at IS NULL))
);
