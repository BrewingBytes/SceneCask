-- R02 0002: TMDB-backed catalog. Provider IDs are unique but never public primary keys.
-- Shows, seasons and episodes are never deleted; removed seasons and episodes are archived so
-- history survives.

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
    EXECUTE FUNCTION reject_update('catalog_revision', 'revision must advance with every change and never decrease');

CREATE TABLE seasons (
    id uuid PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Provider identity, so numbering corrections match seasons by tmdb_id, never by number.
    tmdb_id bigint NOT NULL UNIQUE CHECK (tmdb_id > 0),
    show_id uuid NOT NULL REFERENCES shows (id) ON DELETE RESTRICT,
    -- Season 0 holds specials.
    number integer NOT NULL CHECK (number >= 0),
    -- Set when the provider removes the season. Its episodes are archived with it; the row stays
    -- because episodes and progress reference it.
    archived_at timestamptz,
    -- Unique show+number among active seasons, so a season the provider recreates under a new
    -- tmdb_id can take the archived season's number. Deferrable so an import can swap corrected
    -- numbers in one transaction. An exclusion constraint cannot be an ON CONFLICT arbiter;
    -- imports lock the show row and upsert on tmdb_id.
    CONSTRAINT seasons_active_show_number_key
        EXCLUDE USING btree (show_id WITH =, number WITH =) WHERE (archived_at IS NULL)
        DEFERRABLE INITIALLY IMMEDIATE,
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
    -- An IANA zone name; the trigger below rejects names PostgreSQL cannot resolve.
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

-- Release state is computed with `AT TIME ZONE release_timezone`, so one unresolvable name would
-- fail every read of the show. Resolve it on write instead.
CREATE FUNCTION episodes_release_timezone_valid() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    PERFORM now() AT TIME ZONE NEW.release_timezone;
    RETURN NEW;
EXCEPTION WHEN invalid_parameter_value THEN
    RAISE EXCEPTION 'release_timezone is not a recognized time zone'
        USING ERRCODE = 'check_violation', TABLE = TG_TABLE_NAME, COLUMN = 'release_timezone';
END;
$$;
CREATE TRIGGER episodes_release_timezone_valid
    BEFORE INSERT OR UPDATE OF release_timezone ON episodes
    FOR EACH ROW WHEN (NEW.release_timezone IS NOT NULL)
    EXECUTE FUNCTION episodes_release_timezone_valid();

-- Catalog rows never move to another show, so progress and history stay with their show through
-- numbering corrections. An episode may still move between seasons of its show.
CREATE TRIGGER seasons_show_unchanged
    BEFORE UPDATE OF show_id ON seasons
    FOR EACH ROW WHEN (NEW.show_id <> OLD.show_id)
    EXECUTE FUNCTION reject_update('show_id', 'catalog rows cannot move to another show');
CREATE TRIGGER episodes_show_unchanged
    BEFORE UPDATE OF show_id ON episodes
    FOR EACH ROW WHEN (NEW.show_id <> OLD.show_id)
    EXECUTE FUNCTION reject_update('show_id', 'catalog rows cannot move to another show');

-- Any change to which seasons or episodes exist, their order, release dates or archival advances
-- the show's catalog_revision, so a catch-up preview built on the old catalog fails as
-- PREVIEW_STALE. Titles, overviews and artwork do not affect previews. The triggers run once per
-- statement over its transition tables, so a bulk import bumps each show once rather than once
-- per row. Transition tables cannot be combined with a column list, so the update triggers
-- compare the relevant columns here. Imports lock the show row first, so this update does not add
-- a new lock order.
CREATE FUNCTION advance_catalog_revision() RETURNS trigger
LANGUAGE plpgsql AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        UPDATE shows SET catalog_revision = catalog_revision + 1
        WHERE id IN (SELECT show_id FROM new_rows);
    ELSIF TG_TABLE_NAME = 'seasons' THEN
        UPDATE shows SET catalog_revision = catalog_revision + 1
        WHERE id IN (
            SELECT n.show_id FROM new_rows n JOIN old_rows o USING (id)
            WHERE (n.number, n.archived_at) IS DISTINCT FROM (o.number, o.archived_at));
    ELSE
        UPDATE shows SET catalog_revision = catalog_revision + 1
        WHERE id IN (
            SELECT n.show_id FROM new_rows n JOIN old_rows o USING (id)
            WHERE (n.season_id, n.number, n.air_date, n.release_timezone, n.archived_at)
                IS DISTINCT FROM (o.season_id, o.number, o.air_date, o.release_timezone, o.archived_at));
    END IF;
    RETURN NULL;
END;
$$;
CREATE TRIGGER seasons_insert_advances_catalog
    AFTER INSERT ON seasons REFERENCING NEW TABLE AS new_rows
    FOR EACH STATEMENT EXECUTE FUNCTION advance_catalog_revision();
CREATE TRIGGER seasons_update_advances_catalog
    AFTER UPDATE ON seasons REFERENCING OLD TABLE AS old_rows NEW TABLE AS new_rows
    FOR EACH STATEMENT EXECUTE FUNCTION advance_catalog_revision();
CREATE TRIGGER episodes_insert_advances_catalog
    AFTER INSERT ON episodes REFERENCING NEW TABLE AS new_rows
    FOR EACH STATEMENT EXECUTE FUNCTION advance_catalog_revision();
CREATE TRIGGER episodes_update_advances_catalog
    AFTER UPDATE ON episodes REFERENCING OLD TABLE AS old_rows NEW TABLE AS new_rows
    FOR EACH STATEMENT EXECUTE FUNCTION advance_catalog_revision();
