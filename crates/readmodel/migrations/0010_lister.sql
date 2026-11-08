-- A fifth role (G9): the lister service lists the markets of recurring series, and nothing else.
ALTER TABLE users DROP CONSTRAINT users_role_check;
ALTER TABLE users ADD CONSTRAINT users_role_check CHECK (role IN ('trader', 'admin', 'resolver', 'surveillance', 'lister'));
