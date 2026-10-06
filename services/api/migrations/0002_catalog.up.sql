-- R02 0002: TMDB-backed catalog. Provider IDs are unique but never public primary keys.
-- Shows, seasons and episodes are never deleted; removed episodes are archived so history survives.

CREATE TABLE shows (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tmdb_id bigint NOT NULL UNIQUE CHECK (tmdb_id > 0),
    title text NOT NULL CHECK (title <> ''),
    first_air_year smallint CHECK (first_air_year BETWEEN 1900 AND 2999),
    genres jsonb NOT NULL DEFAULT '[]' CHECK (jsonb_typeof(genres) = 'array'),
    synopsis text,
    poster_path text,
    status text NOT NULL DEFAULT 'unknown'
        CHECK (status IN ('returning', 'ended', 'canceled', 'unknown')),
    catalog_revision bigint NOT NULL DEFAULT 0 CHECK (catalog_revision >= 0),
    fetched_at timestamptz NOT NULL,
    complete_import boolean NOT NULL DEFAULT false,
    created_at timestamptz NOT NULL DEFAULT now()
);
-- Metadata refresh scans the stalest catalog rows first.
CREATE INDEX shows_fetched_idx ON shows (fetched_at);
CREATE TRIGGER shows_catalog_revision_forward
    BEFORE UPDATE OF catalog_revision ON shows
    FOR EACH ROW EXECUTE FUNCTION reject_revision_decrease('catalog_revision');

CREATE TABLE seasons (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    show_id uuid NOT NULL REFERENCES shows (id) ON DELETE RESTRICT,
    -- Season 0 holds specials.
    number integer NOT NULL CHECK (number >= 0),
    UNIQUE (show_id, number)
);

CREATE TABLE episodes (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tmdb_id bigint NOT NULL UNIQUE CHECK (tmdb_id > 0),
    season_id uuid NOT NULL REFERENCES seasons (id) ON DELETE RESTRICT,
    number integer NOT NULL CHECK (number >= 0),
    title text,
    overview text,
    still_path text,
    air_date date,
    release_timezone text CHECK (release_timezone <> ''),
    archived_at timestamptz,
    -- Unique season+number among active episodes. Deferrable so an import can swap
    -- corrected numbers in one transaction; archived rows keep their old number.
    CONSTRAINT episodes_active_season_number_key
        EXCLUDE USING btree (season_id WITH =, number WITH =) WHERE (archived_at IS NULL)
        DEFERRABLE INITIALLY IMMEDIATE
);
CREATE INDEX episodes_season_order_idx ON episodes (season_id, number, id);
