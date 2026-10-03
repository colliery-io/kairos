-- Reverse of COLLIERY-T-3105: removes the read tokens of the repositories.
-- The builder then fetches each repository with no credential, so it
-- cannot read a private repository until an admin sets the token again.
DROP TABLE IF EXISTS repository_credentials;
