use rusqlite::Connection;

pub fn run_migrations(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS activities (
            id              TEXT PRIMARY KEY,
            source          TEXT NOT NULL,
            source_id       TEXT NOT NULL,
            kind            TEXT NOT NULL,
            title           TEXT NOT NULL,
            description     TEXT,
            url             TEXT NOT NULL,
            project         TEXT,
            occurred_at     TEXT NOT NULL,
            metadata        TEXT NOT NULL,
            synced_at       TEXT NOT NULL,
            UNIQUE(source, source_id)
        );

        CREATE TABLE IF NOT EXISTS sync_cursors (
            source      TEXT PRIMARY KEY,
            cursor      TEXT NOT NULL,
            last_sync   TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS llm_cache (
            cache_key   TEXT PRIMARY KEY,
            summary     TEXT NOT NULL,
            created_at  TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_activities_occurred_at
            ON activities(occurred_at);

        CREATE INDEX IF NOT EXISTS idx_activities_source_occurred_at
            ON activities(source, occurred_at);

        CREATE TABLE IF NOT EXISTS tokens (
            key     TEXT PRIMARY KEY,
            value   TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS sync_status (
            source      TEXT PRIMARY KEY,
            ok          INTEGER NOT NULL,
            message     TEXT,
            at          TEXT NOT NULL
        );

        -- Days whose history has been fully fetched (backfill / period refresh).
        CREATE TABLE IF NOT EXISTS fetched_days (
            day         TEXT PRIMARY KEY,
            fetched_at  TEXT NOT NULL
        );

        -- GitHub reviews used to come from the events feed, keyed by event id; they now come
        -- from search as one `review:<repo>:<pr>:<day>` row per PR per day. Drop the old rows
        -- so the same review isn't counted under both schemes.
        DELETE FROM activities
         WHERE source = 'github' AND kind = 'pr_reviewed' AND source_id NOT LIKE 'review:%';
        ",
    )
}
