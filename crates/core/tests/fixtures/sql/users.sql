-- Schema for the user accounts module.
--
-- Tables, indexes, and the core functions exposed via PostgREST. The
-- schema is created from scratch by the migration runner; it is safe to
-- run on an empty database. Re-running it requires a manual DROP.

CREATE SCHEMA IF NOT EXISTS app;

/*
 * Canonical user table.
 *
 * `id` is the lowercased local part of the email (see app.make_id).
 * `email` carries a UNIQUE constraint so the application layer can
 * surface a meaningful 409 instead of a generic 500 on conflicts.
 */
CREATE TABLE app.users (
    id          text PRIMARY KEY,
    email       text NOT NULL UNIQUE,
    name        text NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now()
);

-- M:N tags. CASCADE on delete so removing a user wipes the join rows.
CREATE TABLE app.user_tags (
    user_id  text NOT NULL REFERENCES app.users(id) ON DELETE CASCADE,
    tag      text NOT NULL,
    PRIMARY KEY (user_id, tag)
);

-- Lookup by tag is the dominant access pattern for the listing endpoint.
CREATE INDEX user_tags_tag_idx ON app.user_tags (tag);

-- Stable id derivation. IMMUTABLE so it can be used in indexed expressions
-- if we ever decide to materialise it.
CREATE FUNCTION app.make_id(p_email text) RETURNS text
LANGUAGE sql IMMUTABLE AS $$
    SELECT lower(split_part(p_email, '@', 1))
$$;

/*
 * Create a new user.
 *
 * Emits SQLSTATE 23505 (unique_violation) when the email is already taken
 * so the application layer can pattern-match without parsing the message.
 */
CREATE FUNCTION app.create_user(p_email text, p_name text)
RETURNS app.users
LANGUAGE plpgsql AS $$
DECLARE
    v_id   text;
    v_row  app.users%ROWTYPE;
BEGIN
    -- Pre-check is best-effort; the UNIQUE index is the actual guard.
    v_id := app.make_id(p_email);
    IF EXISTS (SELECT 1 FROM app.users WHERE email = p_email) THEN
        RAISE EXCEPTION 'email % already registered', p_email USING ERRCODE = '23505';
    END IF;
    INSERT INTO app.users (id, email, name)
    VALUES (v_id, p_email, p_name)
    RETURNING * INTO v_row;
    RETURN v_row;
END
$$;

-- Listing endpoint. STABLE because it never writes; the query planner
-- can lift it out of subqueries when it's safe to do so.
CREATE FUNCTION app.users_with_tag(p_tag text)
RETURNS SETOF app.users
LANGUAGE sql STABLE AS $$
    SELECT u.*
    FROM app.users u
    JOIN app.user_tags t ON t.user_id = u.id
    WHERE t.tag = p_tag
    ORDER BY u.created_at DESC
$$;

-- Convenience view used by the admin dashboard. Drop + recreate is fine
-- because no other object depends on it.
CREATE VIEW app.recent_users AS
    SELECT id, email, name
    FROM app.users
    WHERE created_at > now() - interval '7 days';
