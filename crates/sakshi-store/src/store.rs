use std::path::{Path, PathBuf};
use std::time::Duration;

use jiff::{Timestamp, Unit};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row, params};
use sakshi_core::{
    Frame, MeetingDir, MeetingInfo, Segment, Speaker, Summary, Track, Transcript, VideoPause, Word,
    read_json, write_json_atomic,
};
use serde::{Deserialize, Serialize};

use crate::{StoreError, schema, summaries};

/// A processed meeting as listed by the index.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MeetingRecord {
    pub id: String,
    pub title: String,
    pub folder: PathBuf,
    /// RFC 3339, when known.
    pub started_at: Option<String>,
    /// RFC 3339 time the meeting was last indexed.
    pub indexed_at: String,
    pub duration_ms: u64,
    pub languages: Vec<String>,
    /// Speaker labels in order of first appearance.
    pub speakers: Vec<String>,
    pub segment_count: u32,
    pub frame_count: u32,
    /// Where the recorded window's video paused because the window was out
    /// of view, in time order; empty for a display or a recording made
    /// elsewhere.
    pub video_pauses: Vec<VideoPause>,
}

#[derive(Debug, Clone, Default)]
pub struct MeetingQuery {
    /// Meetings starting at or after this time (or indexed then, if the
    /// start time is unknown).
    pub since: Option<Timestamp>,
    /// Meetings starting before this time.
    pub until: Option<Timestamp>,
    /// Matches the title or any transcript text.
    pub text: Option<String>,
    pub limit: u32,
}

/// A half-open time range `[start_ms, end_ms)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeRange {
    pub start_ms: u64,
    pub end_ms: u64,
}

impl TimeRange {
    pub const ALL: TimeRange = TimeRange {
        start_ms: 0,
        end_ms: u64::MAX,
    };
}

/// A segment matching a search, with its meeting.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchHit {
    pub meeting_id: String,
    pub meeting_title: String,
    pub started_at: Option<String>,
    pub segment_id: u32,
    pub start_ms: u64,
    pub end_ms: u64,
    pub track: Track,
    pub speaker: Speaker,
    pub text: String,
}

pub struct Store {
    conn: Connection,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store").finish_non_exhaustive()
    }
}

/// SQLite stores integers as i64; times in milliseconds always fit.
pub(crate) fn to_sql(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

pub(crate) fn from_sql(v: i64) -> u64 {
    u64::try_from(v).unwrap_or(0)
}

/// RFC 3339 in UTC to whole seconds, so stored times sort as strings.
pub(crate) fn format_time(t: Timestamp) -> String {
    t.round(Unit::Second).unwrap_or(t).to_string()
}

pub(crate) fn configure(conn: &Connection) -> Result<(), StoreError> {
    conn.busy_timeout(Duration::from_secs(5))?;
    conn.pragma_update(None, "foreign_keys", true)?;
    Ok(())
}

pub(crate) fn check_version(conn: &Connection) -> Result<(), StoreError> {
    let found = schema::version(conn)?;
    match found.cmp(&schema::VERSION) {
        std::cmp::Ordering::Equal => Ok(()),
        std::cmp::Ordering::Greater => Err(StoreError::NewerSchema {
            found,
            known: schema::VERSION,
        }),
        std::cmp::Ordering::Less => Err(StoreError::OutdatedSchema {
            found,
            known: schema::VERSION,
        }),
    }
}

/// Upgrades the database at `path` if `conn` shows an older schema, and
/// says whether it did. Without this, after updating Sakshi the MCP server
/// and `sakshi-cli meetings`, which only read, would refuse the database
/// until something indexed a meeting.
pub(crate) fn upgrade_if_outdated(conn: &Connection, path: &Path) -> Result<bool, StoreError> {
    match check_version(conn) {
        Err(StoreError::OutdatedSchema { found, .. }) => {
            tracing::info!(found, "upgrading the database written by an older Sakshi");
            Store::open(path)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

const MEETING_COLUMNS: &str = "id, title, folder, started_at, indexed_at, duration_ms, \
     languages, speakers, segment_count, frame_count, video_pauses";

fn meeting_from_row(row: &Row<'_>) -> rusqlite::Result<MeetingRecord> {
    let list = |i: usize| -> rusqlite::Result<Vec<String>> {
        Ok(serde_json::from_str(&row.get::<_, String>(i)?).unwrap_or_default())
    };
    Ok(MeetingRecord {
        id: row.get(0)?,
        title: row.get(1)?,
        folder: PathBuf::from(row.get::<_, String>(2)?),
        started_at: row.get(3)?,
        indexed_at: row.get(4)?,
        duration_ms: from_sql(row.get(5)?),
        languages: list(6)?,
        speakers: list(7)?,
        segment_count: row.get(8)?,
        frame_count: row.get(9)?,
        video_pauses: serde_json::from_str(&row.get::<_, String>(10)?).unwrap_or_default(),
    })
}

/// The video pauses a Sakshi recording kept in `capture/capture.json`.
/// Recordings made elsewhere have none; an unreadable file is logged and
/// treated as none, since the transcript is still worth indexing.
fn video_pauses(dir: &MeetingDir) -> Vec<VideoPause> {
    #[derive(Default, Deserialize)]
    #[serde(default)]
    struct Capture {
        stats: Option<Stats>,
    }
    #[derive(Default, Deserialize)]
    #[serde(default)]
    struct Stats {
        video_pauses: Vec<VideoPause>,
    }

    let path = dir.capture_json();
    if !path.exists() {
        return Vec::new();
    }
    match read_json::<Capture>(&path) {
        Ok(capture) => {
            let mut pauses = capture.stats.map(|s| s.video_pauses).unwrap_or_default();
            pauses.sort_by_key(|p| p.from_ms);
            pauses
        }
        Err(e) => {
            tracing::warn!("ignoring the video pauses in {}: {e}", path.display());
            Vec::new()
        }
    }
}

fn parse_track(s: &str) -> Track {
    if s == "mic" {
        Track::Mic
    } else {
        Track::System
    }
}

fn parse_speaker(s: &str) -> Speaker {
    s.parse().unwrap_or(Speaker::Remote(1))
}

/// An FTS5 trigram query matching `text` as a substring, or `None` when
/// the text is too short for trigrams and a LIKE scan must be used.
fn trigram_phrase(text: &str) -> Option<String> {
    (text.chars().count() >= 3).then(|| format!("\"{}\"", text.replace('"', "\"\"")))
}

/// A LIKE pattern matching `text` anywhere, with wildcards escaped.
fn like_pattern(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

impl Store {
    /// Opens (creating if needed) the database for indexing, in WAL mode,
    /// and brings its schema up to date.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| StoreError::io(parent, e))?;
        }
        let mut conn = Connection::open(path)?;
        configure(&conn)?;
        let mode: String =
            conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            tracing::warn!(%mode, "SQLite did not switch to WAL mode");
        }
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        schema::migrate(&mut conn)?;
        Ok(Self { conn })
    }

    /// Opens an existing database for queries only. One written by an older
    /// Sakshi is upgraded first (see [`upgrade_if_outdated`]).
    pub fn open_read_only(path: &Path) -> Result<Self, StoreError> {
        if !path.exists() {
            return Err(StoreError::NoDatabase {
                path: path.to_path_buf(),
            });
        }
        let connect = || -> Result<Connection, StoreError> {
            let conn = Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            configure(&conn)?;
            conn.pragma_update(None, "query_only", true)?;
            Ok(conn)
        };
        let mut conn = connect()?;
        if upgrade_if_outdated(&conn, path)? {
            conn = connect()?;
        }
        check_version(&conn)?;
        Ok(Self { conn })
    }

    /// Indexes a processed meeting folder, replacing what was indexed for
    /// it before. Assigns the meeting an id on first sight and records it
    /// in `meeting.json`, so the folder keeps its id if it moves.
    pub fn index_meeting(&mut self, dir: &MeetingDir) -> Result<MeetingRecord, StoreError> {
        let folder = std::path::absolute(dir.path()).map_err(|e| StoreError::io(dir.path(), e))?;
        let folder_text = folder.to_string_lossy().into_owned();
        if !dir.transcript_json().exists() {
            return Err(StoreError::NotProcessed { dir: folder });
        }
        let transcript: Transcript = read_json(&dir.transcript_json())?;
        let frames: Vec<Frame> = if dir.frames_json().exists() {
            read_json(&dir.frames_json())?
        } else {
            Vec::new()
        };
        let mut info: MeetingInfo = if dir.info_json().exists() {
            read_json(&dir.info_json())?
        } else {
            MeetingInfo::default()
        };

        let known_for_folder: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM meetings WHERE folder = ?1",
                [&folder_text],
                |r| r.get(0),
            )
            .optional()?;
        let id = info
            .id
            .clone()
            .filter(|id| uuid::Uuid::parse_str(id).is_ok())
            .or(known_for_folder)
            .unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
        if info.id.as_deref() != Some(id.as_str()) {
            info.id = Some(id.clone());
            write_json_atomic(&dir.info_json(), &info)?;
        }

        let mut speakers: Vec<String> = Vec::new();
        for segment in &transcript.segments {
            let label = segment.speaker.to_string();
            if !speakers.contains(&label) {
                speakers.push(label);
            }
        }
        let started_at = info
            .started_at
            .as_deref()
            .and_then(|s| match s.parse::<Timestamp>() {
                Ok(t) => Some(format_time(t)),
                Err(e) => {
                    tracing::warn!(started_at = s, "ignoring unparseable start time: {e}");
                    None
                }
            });
        let record = MeetingRecord {
            id: id.clone(),
            title: info
                .title
                .clone()
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| {
                    folder
                        .file_name()
                        .map_or_else(|| id.clone(), |n| n.to_string_lossy().into_owned())
                }),
            folder: folder.clone(),
            started_at,
            indexed_at: format_time(Timestamp::now()),
            duration_ms: transcript.duration_ms,
            languages: transcript.languages.clone(),
            speakers,
            segment_count: u32::try_from(transcript.segments.len()).unwrap_or(u32::MAX),
            frame_count: u32::try_from(frames.len()).unwrap_or(u32::MAX),
            video_pauses: video_pauses(dir),
        };

        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM meetings WHERE folder = ?1 AND id <> ?2",
            params![folder_text, id],
        )?;
        tx.execute(
            "INSERT INTO meetings (id, title, folder, started_at, indexed_at, duration_ms,
                                   languages, speakers, segment_count, frame_count,
                                   video_pauses)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT (id) DO UPDATE SET
                title = excluded.title, folder = excluded.folder,
                started_at = excluded.started_at, indexed_at = excluded.indexed_at,
                duration_ms = excluded.duration_ms, languages = excluded.languages,
                speakers = excluded.speakers, segment_count = excluded.segment_count,
                frame_count = excluded.frame_count, video_pauses = excluded.video_pauses",
            params![
                record.id,
                record.title,
                folder_text,
                record.started_at,
                record.indexed_at,
                to_sql(record.duration_ms),
                serde_json::to_string(&record.languages).expect("strings serialize"),
                serde_json::to_string(&record.speakers).expect("strings serialize"),
                record.segment_count,
                record.frame_count,
                serde_json::to_string(&record.video_pauses).expect("pauses serialize"),
            ],
        )?;
        for table in ["segments", "frames", "segment_search"] {
            tx.execute(&format!("DELETE FROM {table} WHERE meeting_id = ?1"), [&id])?;
        }
        {
            let mut insert = tx.prepare(
                "INSERT INTO segments (meeting_id, id, start_ms, end_ms, track, speaker, text, words)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;
            let mut search = tx.prepare(
                "INSERT INTO segment_search (text, meeting_id, segment_id) VALUES (?1, ?2, ?3)",
            )?;
            for s in &transcript.segments {
                insert.execute(params![
                    id,
                    s.id,
                    to_sql(s.start_ms),
                    to_sql(s.end_ms),
                    s.track.as_str(),
                    s.speaker.to_string(),
                    s.text,
                    serde_json::to_string(&s.words).expect("words serialize"),
                ])?;
                search.execute(params![s.text, id, s.id])?;
            }
            let mut insert = tx.prepare(
                "INSERT INTO frames (meeting_id, at_ms, file, width, height)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for f in &frames {
                insert.execute(params![id, to_sql(f.at_ms), f.file, f.width, f.height])?;
            }
        }
        summaries::sync(&tx, &id, dir)?;
        tx.commit()?;
        Ok(record)
    }

    /// Meetings, most recent first.
    pub fn meetings(&self, query: &MeetingQuery) -> Result<Vec<MeetingRecord>, StoreError> {
        let text = query
            .text
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty());
        let like = text.map(like_pattern);
        let phrase = text.and_then(trigram_phrase);
        let transcript_match = if phrase.is_some() {
            "id IN (SELECT meeting_id FROM segment_search WHERE segment_search MATCH ?5)"
        } else {
            // ?5 (always NULL here) keeps the parameter count the same.
            "id IN (SELECT meeting_id FROM segments WHERE text LIKE ?3 ESCAPE '\\' AND ?5 IS NULL)"
        };
        let sql = format!(
            "SELECT {MEETING_COLUMNS} FROM meetings
             WHERE (?1 IS NULL OR COALESCE(started_at, indexed_at) >= ?1)
               AND (?2 IS NULL OR COALESCE(started_at, indexed_at) < ?2)
               AND (?3 IS NULL OR title LIKE ?3 ESCAPE '\\' OR {transcript_match})
             ORDER BY COALESCE(started_at, indexed_at) DESC, id DESC
             LIMIT ?4"
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(
            params![
                query.since.map(format_time),
                query.until.map(format_time),
                like,
                query.limit,
                phrase,
            ],
            meeting_from_row,
        )?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn meeting(&self, id: &str) -> Result<Option<MeetingRecord>, StoreError> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {MEETING_COLUMNS} FROM meetings WHERE id = ?1"),
                [id],
                meeting_from_row,
            )
            .optional()?)
    }

    /// The meeting, or [`StoreError::UnknownMeeting`].
    pub fn require_meeting(&self, id: &str) -> Result<MeetingRecord, StoreError> {
        self.meeting(id)?
            .ok_or_else(|| StoreError::UnknownMeeting(id.to_owned()))
    }

    /// Segments overlapping `range` with an id above `after`, in id (and so
    /// time) order.
    pub fn segments(
        &self,
        meeting_id: &str,
        range: TimeRange,
        after: Option<u32>,
        limit: u32,
    ) -> Result<Vec<Segment>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, start_ms, end_ms, track, speaker, text, words FROM segments
             WHERE meeting_id = ?1 AND end_ms > ?2 AND start_ms < ?3 AND id > ?4
             ORDER BY id LIMIT ?5",
        )?;
        let rows = stmt.query_map(
            params![
                meeting_id,
                to_sql(range.start_ms),
                to_sql(range.end_ms),
                after.map_or(-1, i64::from),
                limit
            ],
            |row| {
                let words: Vec<Word> =
                    serde_json::from_str(&row.get::<_, String>(6)?).unwrap_or_default();
                Ok(Segment {
                    id: row.get(0)?,
                    start_ms: from_sql(row.get(1)?),
                    end_ms: from_sql(row.get(2)?),
                    track: parse_track(&row.get::<_, String>(3)?),
                    speaker: parse_speaker(&row.get::<_, String>(4)?),
                    text: row.get(5)?,
                    words,
                })
            },
        )?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Key frames taken within `range`, in time order.
    pub fn frames(&self, meeting_id: &str, range: TimeRange) -> Result<Vec<Frame>, StoreError> {
        let mut stmt = self.conn.prepare(
            "SELECT at_ms, file, width, height FROM frames
             WHERE meeting_id = ?1 AND at_ms >= ?2 AND at_ms < ?3 ORDER BY at_ms",
        )?;
        let rows = stmt.query_map(
            params![meeting_id, to_sql(range.start_ms), to_sql(range.end_ms)],
            frame_from_row,
        )?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The key frame on screen at `at_ms`: the latest one taken at or before
    /// it, or the first frame if `at_ms` precedes them all.
    pub fn frame_at(&self, meeting_id: &str, at_ms: u64) -> Result<Option<Frame>, StoreError> {
        let before = self
            .conn
            .query_row(
                "SELECT at_ms, file, width, height FROM frames
                 WHERE meeting_id = ?1 AND at_ms <= ?2 ORDER BY at_ms DESC LIMIT 1",
                params![meeting_id, to_sql(at_ms)],
                frame_from_row,
            )
            .optional()?;
        if before.is_some() {
            return Ok(before);
        }
        Ok(self
            .conn
            .query_row(
                "SELECT at_ms, file, width, height FROM frames
                 WHERE meeting_id = ?1 ORDER BY at_ms LIMIT 1",
                [meeting_id],
                frame_from_row,
            )
            .optional()?)
    }

    /// Segments containing `text` (case-insensitive), best matches first.
    pub fn search(&self, text: &str, limit: u32) -> Result<Vec<SearchHit>, StoreError> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(Vec::new());
        }
        let (sql, arg) = match trigram_phrase(text) {
            Some(phrase) => (
                "SELECT s.meeting_id, m.title, m.started_at, s.id, s.start_ms, s.end_ms,
                        s.track, s.speaker, s.text
                 FROM segment_search f
                 JOIN segments s ON s.meeting_id = f.meeting_id AND s.id = f.segment_id
                 JOIN meetings m ON m.id = s.meeting_id
                 WHERE segment_search MATCH ?1
                 ORDER BY f.rank LIMIT ?2",
                phrase,
            ),
            None => (
                "SELECT s.meeting_id, m.title, m.started_at, s.id, s.start_ms, s.end_ms,
                        s.track, s.speaker, s.text
                 FROM segments s JOIN meetings m ON m.id = s.meeting_id
                 WHERE s.text LIKE ?1 ESCAPE '\\'
                 ORDER BY COALESCE(m.started_at, m.indexed_at) DESC, s.start_ms LIMIT ?2",
                like_pattern(text),
            ),
        };
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map(params![arg, limit], |row| {
            Ok(SearchHit {
                meeting_id: row.get(0)?,
                meeting_title: row.get(1)?,
                started_at: row.get(2)?,
                segment_id: row.get(3)?,
                start_ms: from_sql(row.get(4)?),
                end_ms: from_sql(row.get(5)?),
                track: parse_track(&row.get::<_, String>(6)?),
                speaker: parse_speaker(&row.get::<_, String>(7)?),
                text: row.get(8)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Summaries saved for a meeting, oldest first.
    pub fn summaries(&self, meeting_id: &str) -> Result<Vec<Summary>, StoreError> {
        let meeting = self.require_meeting(meeting_id)?;
        summaries::load_all(&self.conn, &meeting)
    }
}

fn frame_from_row(row: &Row<'_>) -> rusqlite::Result<Frame> {
    Ok(Frame {
        at_ms: from_sql(row.get(0)?),
        file: row.get(1)?,
        width: row.get(2)?,
        height: row.get(3)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_helpers_escape_input() {
        assert_eq!(trigram_phrase("ab"), None);
        assert_eq!(trigram_phrase("say \"hi\"").unwrap(), "\"say \"\"hi\"\"\"");
        assert_eq!(like_pattern("50%_a\\"), "%50\\%\\_a\\\\%");
    }

    #[test]
    fn times_are_stored_to_the_second() {
        let t: Timestamp = "2026-10-26T13:00:00.75Z".parse().unwrap();
        assert_eq!(format_time(t), "2026-10-26T13:00:01Z");
    }
}
