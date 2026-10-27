//! Schema migrations, tracked with `PRAGMA user_version`.

use rusqlite::Connection;

use crate::StoreError;

/// Each entry upgrades the schema by one version. Never edit a shipped
/// migration; append a new one.
const MIGRATIONS: &[&str] = &[
    // 1: meetings, segments, frames, summaries, and trigram search.
    r#"
    CREATE TABLE meetings (
        id            TEXT PRIMARY KEY,
        folder        TEXT NOT NULL UNIQUE,
        title         TEXT NOT NULL,
        started_at    TEXT,
        indexed_at    TEXT NOT NULL,
        duration_ms   INTEGER NOT NULL,
        languages     TEXT NOT NULL,
        speakers      TEXT NOT NULL,
        segment_count INTEGER NOT NULL,
        frame_count   INTEGER NOT NULL
    ) STRICT;

    CREATE TABLE segments (
        meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
        id         INTEGER NOT NULL,
        start_ms   INTEGER NOT NULL,
        end_ms     INTEGER NOT NULL,
        track      TEXT NOT NULL,
        speaker    TEXT NOT NULL,
        text       TEXT NOT NULL,
        words      TEXT NOT NULL,
        PRIMARY KEY (meeting_id, id)
    ) STRICT;
    CREATE INDEX segments_by_time ON segments (meeting_id, start_ms);

    CREATE TABLE frames (
        meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
        at_ms      INTEGER NOT NULL,
        file       TEXT NOT NULL,
        width      INTEGER NOT NULL,
        height     INTEGER NOT NULL,
        PRIMARY KEY (meeting_id, at_ms)
    ) STRICT;

    CREATE TABLE summaries (
        id           TEXT PRIMARY KEY,
        meeting_id   TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
        title        TEXT,
        author_model TEXT,
        created_at   TEXT NOT NULL,
        file         TEXT NOT NULL
    ) STRICT;
    CREATE INDEX summaries_by_meeting ON summaries (meeting_id, created_at);

    -- Trigram matching finds substrings in any script, including Devanagari,
    -- where word tokenizers split on vowel signs.
    CREATE VIRTUAL TABLE segment_search USING fts5 (
        text,
        meeting_id UNINDEXED,
        segment_id UNINDEXED,
        tokenize = 'trigram'
    );
    "#,
    // 2: where a recorded window's video paused, as a JSON list of
    // `VideoPause` from the meeting's `capture/capture.json`.
    r#"
    ALTER TABLE meetings ADD COLUMN video_pauses TEXT NOT NULL DEFAULT '[]';
    "#,
];

pub(crate) const VERSION: i64 = MIGRATIONS.len() as i64;

pub(crate) fn version(conn: &Connection) -> Result<i64, StoreError> {
    Ok(conn.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

/// Brings the schema up to date.
pub(crate) fn migrate(conn: &mut Connection) -> Result<(), StoreError> {
    let current = version(conn)?;
    if current > VERSION {
        return Err(StoreError::NewerSchema {
            found: current,
            known: VERSION,
        });
    }
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i as i64 + 1)?;
        tx.commit()?;
        tracing::info!(version = i + 1, "database schema upgraded");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MeetingQuery, Store, SummaryWriter};

    /// A database as Sakshi 0.1.0 left it: schema 1 with one meeting.
    fn version_1(path: &std::path::Path) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute(
            "INSERT INTO meetings VALUES ('m1', 'Standup', '/tmp/m1', NULL,
             '2026-10-26T10:00:00Z', 60000, '[]', '[]', 0, 0)",
            [],
        )
        .unwrap();
    }

    #[test]
    fn readers_and_the_summary_writer_upgrade_an_older_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sakshi.db");
        version_1(&path);

        let store = Store::open_read_only(&path).unwrap();
        let meetings = store
            .meetings(&MeetingQuery {
                limit: 5,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(meetings.len(), 1);
        assert!(meetings[0].video_pauses.is_empty());
        drop(store);
        assert_eq!(version(&Connection::open(&path).unwrap()).unwrap(), VERSION);

        let other = dir.path().join("other.db");
        version_1(&other);
        SummaryWriter::open(&other).unwrap();
        let conn = Connection::open(&other).unwrap();
        assert_eq!(version(&conn).unwrap(), VERSION);
    }
}
