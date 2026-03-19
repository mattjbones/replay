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
        ",
    )
}
