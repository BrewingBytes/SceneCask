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
    FOR EACH ROW WHEN (NEW.catalog_revision < OLD.catalog_revision)
    EXECUTE FUNCTION reject_revision_decrease('catalog_revision');

CREATE TABLE seasons (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Provider identity, so numbering corrections match seasons by tmdb_id, never by number.
    tmdb_id bigint NOT NULL UNIQUE CHECK (tmdb_id > 0),
    show_id uuid NOT NULL REFERENCES shows (id) ON DELETE RESTRICT,
    -- Season 0 holds specials.
    number integer NOT NULL CHECK (number >= 0),
    -- Deferrable so an import can swap corrected season numbers in one transaction. A deferrable
    -- constraint cannot be an ON CONFLICT arbiter; imports lock the show row and upsert on tmdb_id.
    CONSTRAINT seasons_show_number_key UNIQUE (show_id, number) DEFERRABLE INITIALLY IMMEDIATE,
    -- Target for the episodes (show_id, season_id) foreign key.
    UNIQUE (show_id, id)
);

CREATE TABLE episodes (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    tmdb_id bigint NOT NULL UNIQUE CHECK (tmdb_id > 0),
    -- Denormalized from the season so rows pairing a show with an episode (previews, activity)
    -- can reference (show_id, id) and PostgreSQL checks that the episode belongs to the show.
    show_id uuid NOT NULL,
    season_id uuid NOT NULL,
    number integer NOT NULL CHECK (number >= 0),
    title text,
    overview text,
    still_path text,
    air_date date,
    release_timezone text CHECK (release_timezone <> ''),
    archived_at timestamptz,
    FOREIGN KEY (show_id, season_id) REFERENCES seasons (show_id, id) ON DELETE RESTRICT,
    UNIQUE (show_id, id),
    -- Unique season+number among active episodes. Deferrable so an import can swap
    -- corrected numbers in one transaction; archived rows keep their old number. Its index
    -- also serves season/episode ordering of active episodes. An exclusion constraint cannot be
    -- an ON CONFLICT arbiter, so imports upsert episodes on tmdb_id.
    CONSTRAINT episodes_active_season_number_key
        EXCLUDE USING btree (season_id WITH =, number WITH =) WHERE (archived_at IS NULL)
        DEFERRABLE INITIALLY IMMEDIATE
);

-- Catalog rows never move to another show, so progress and history stay with their show through
-- numbering corrections. An episode may still move between seasons of its show.
CREATE FUNCTION catalog_show_unchanged() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    RAISE EXCEPTION 'catalog rows cannot move to another show'
        USING ERRCODE = 'check_violation', TABLE = TG_TABLE_NAME, COLUMN = 'show_id';
END;
$$;
CREATE TRIGGER seasons_show_unchanged
    BEFORE UPDATE OF show_id ON seasons
    FOR EACH ROW WHEN (NEW.show_id <> OLD.show_id)
    EXECUTE FUNCTION catalog_show_unchanged();
CREATE TRIGGER episodes_show_unchanged
    BEFORE UPDATE OF show_id ON episodes
    FOR EACH ROW WHEN (NEW.show_id <> OLD.show_id)
    EXECUTE FUNCTION catalog_show_unchanged();

-- Any change to which episodes exist, their order, release dates or archival advances the show's
-- catalog_revision, so a catch-up preview built on the old catalog fails as PREVIEW_STALE.
-- Titles, overviews and artwork do not affect previews. Imports lock the show row first, so this
-- update does not add a new lock order.
CREATE FUNCTION advance_catalog_revision() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    UPDATE shows SET catalog_revision = catalog_revision + 1 WHERE id = NEW.show_id;
    RETURN NULL;
END;
$$;
CREATE TRIGGER seasons_insert_advances_catalog
    AFTER INSERT ON seasons
    FOR EACH ROW EXECUTE FUNCTION advance_catalog_revision();
CREATE TRIGGER seasons_update_advances_catalog
    AFTER UPDATE OF number ON seasons
    FOR EACH ROW WHEN (NEW.number <> OLD.number)
    EXECUTE FUNCTION advance_catalog_revision();
CREATE TRIGGER episodes_insert_advances_catalog
    AFTER INSERT ON episodes
    FOR EACH ROW EXECUTE FUNCTION advance_catalog_revision();
CREATE TRIGGER episodes_update_advances_catalog
    AFTER UPDATE OF season_id, number, air_date, release_timezone, archived_at ON episodes
    FOR EACH ROW WHEN ((NEW.season_id, NEW.number, NEW.air_date, NEW.release_timezone, NEW.archived_at)
        IS DISTINCT FROM (OLD.season_id, OLD.number, OLD.air_date, OLD.release_timezone, OLD.archived_at))
    EXECUTE FUNCTION advance_catalog_revision();
