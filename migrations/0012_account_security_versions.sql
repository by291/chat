ALTER TABLE users
    ADD COLUMN credential_version bigint NOT NULL DEFAULT 0 CHECK (credential_version >= 0),
    ADD COLUMN email_version bigint NOT NULL DEFAULT 0 CHECK (email_version >= 0);
