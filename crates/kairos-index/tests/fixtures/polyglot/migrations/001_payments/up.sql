-- The payments of a tenant (KAIROS-T-0350: SQL files are indexed).
CREATE TABLE payments (
    id    UUID PRIMARY KEY,
    day   DATE NOT NULL,
    note  TEXT NOT NULL DEFAULT 'none'
);

CREATE INDEX payments_by_day ON payments (day);
