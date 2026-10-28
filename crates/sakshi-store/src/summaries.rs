//! Summaries written by MCP clients.
//!
//! Each summary is `summaries/<id>.md` in its meeting folder, starting with a
//! small front matter block (JSON values) that carries its metadata, so the
//! index can be rebuilt from the folders alone.

use std::fs;
use std::path::Path;

use jiff::Timestamp;
use rusqlite::{Connection, Transaction, params};
use sakshi_core::{MeetingDir, Summary};

use crate::StoreError;
use crate::store::{MeetingRecord, format_time, open_existing};

/// Largest summary accepted, in bytes of Markdown.
pub const MAX_SUMMARY_BYTES: usize = 256 * 1024;
const MAX_TITLE_CHARS: usize = 200;
const MAX_AUTHOR_CHARS: usize = 100;

/// Saves summaries, and nothing else: the only write the MCP server makes.
pub struct SummaryWriter {
    conn: Connection,
}

impl std::fmt::Debug for SummaryWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SummaryWriter").finish_non_exhaustive()
    }
}

impl SummaryWriter {
    /// Opens an existing database, never creating one. One written by an
    /// older Sakshi is upgraded first, as for reading.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            conn: open_existing(path)?,
        })
    }

    /// Writes `summaries/<id>.md` into the meeting folder and records it.
    pub fn save(
        &mut self,
        meeting_id: &str,
        markdown: &str,
        title: Option<&str>,
        author_model: Option<&str>,
    ) -> Result<Summary, StoreError> {
        if markdown.trim().is_empty() {
            return Err(StoreError::InvalidSummary("the summary is empty".into()));
        }
        if markdown.len() > MAX_SUMMARY_BYTES {
            return Err(StoreError::InvalidSummary(format!(
                "the summary is {} bytes; the limit is {MAX_SUMMARY_BYTES}",
                markdown.len()
            )));
        }
        let clean = |value: Option<&str>, max: usize, what: &str| -> Result<_, StoreError> {
            let value = value.map(str::trim).filter(|v| !v.is_empty());
            match value {
                Some(v) if v.chars().count() > max || v.contains('\n') => {
                    Err(StoreError::InvalidSummary(format!(
                        "{what} must be one line of at most {max} characters"
                    )))
                }
                other => Ok(other.map(str::to_owned)),
            }
        };
        let title = clean(title, MAX_TITLE_CHARS, "title")?;
        let author_model = clean(author_model, MAX_AUTHOR_CHARS, "author_model")?;

        let folder: String = self
            .conn
            .query_row(
                "SELECT folder FROM meetings WHERE id = ?1",
                [meeting_id],
                |r| r.get(0),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    StoreError::UnknownMeeting(meeting_id.to_owned())
                }
                e => e.into(),
            })?;

        let summary = Summary {
            id: uuid::Uuid::now_v7().to_string(),
            meeting_id: meeting_id.to_owned(),
            title,
            author_model,
            created_at: format_time(Timestamp::now()),
            markdown: markdown.to_owned(),
        };
        let file = format!("summaries/{}.md", summary.id);
        write_atomic(&Path::new(&folder).join(&file), &render(&summary))?;
        self.conn.execute(
            "INSERT INTO summaries (id, meeting_id, title, author_model, created_at, file)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                summary.id,
                summary.meeting_id,
                summary.title,
                summary.author_model,
                summary.created_at,
                file
            ],
        )?;
        Ok(summary)
    }
}

fn write_atomic(path: &Path, text: &str) -> Result<(), StoreError> {
    let parent = path.parent().expect("summary paths have a folder");
    fs::create_dir_all(parent).map_err(|e| StoreError::io(parent, e))?;
    let tmp = path.with_extension("md.tmp");
    fs::write(&tmp, text).map_err(|e| StoreError::io(&tmp, e))?;
    fs::rename(&tmp, path).map_err(|e| StoreError::io(path, e))
}

fn json(value: Option<&str>) -> String {
    serde_json::to_string(&value).expect("strings serialize")
}

pub(crate) fn render(s: &Summary) -> String {
    format!(
        "---\nid: {}\nmeeting_id: {}\ntitle: {}\nauthor_model: {}\ncreated_at: {}\n---\n\n{}",
        json(Some(&s.id)),
        json(Some(&s.meeting_id)),
        json(s.title.as_deref()),
        json(s.author_model.as_deref()),
        json(Some(&s.created_at)),
        s.markdown
    )
}

/// Splits a summary file into its metadata and Markdown body.
pub(crate) fn parse(text: &str) -> Option<Summary> {
    let rest = text.strip_prefix("---\n")?;
    let (header, body) = rest.split_once("\n---\n")?;
    let field = |key: &str| -> Option<Option<String>> {
        header
            .lines()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix(": "))
            .and_then(|v| serde_json::from_str(v).ok())
    };
    Some(Summary {
        id: field("id")??,
        meeting_id: field("meeting_id")??,
        title: field("title")?,
        author_model: field("author_model")?,
        created_at: field("created_at")??,
        markdown: body.strip_prefix('\n').unwrap_or(body).to_owned(),
    })
}

/// Records summary files found in the meeting folder and forgets rows whose
/// file is gone.
pub(crate) fn sync(
    tx: &Transaction<'_>,
    meeting_id: &str,
    dir: &MeetingDir,
) -> Result<(), StoreError> {
    let summaries_dir = dir.summaries_dir();
    let mut on_disk = Vec::new();
    if summaries_dir.is_dir() {
        for entry in fs::read_dir(&summaries_dir).map_err(|e| StoreError::io(&summaries_dir, e))? {
            let path = entry.map_err(|e| StoreError::io(&summaries_dir, e))?.path();
            if path.extension().is_some_and(|e| e == "md") {
                on_disk.push(path);
            }
        }
    }
    let mut insert = tx.prepare(
        "INSERT INTO summaries (id, meeting_id, title, author_model, created_at, file)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (id) DO UPDATE SET meeting_id = excluded.meeting_id",
    )?;
    let mut ids = Vec::new();
    for path in on_disk {
        let text = fs::read_to_string(&path).map_err(|e| StoreError::io(&path, e))?;
        let stem = path.file_stem().unwrap_or_default().to_string_lossy();
        let Some(summary) = parse(&text).filter(|s| s.id == stem) else {
            tracing::warn!(file = %path.display(), "skipping summary without valid front matter");
            continue;
        };
        insert.execute(params![
            summary.id,
            meeting_id,
            summary.title,
            summary.author_model,
            summary.created_at,
            format!("summaries/{}.md", summary.id)
        ])?;
        ids.push(summary.id);
    }
    let known: Vec<String> = {
        let mut stmt = tx.prepare("SELECT id FROM summaries WHERE meeting_id = ?1")?;
        stmt.query_map([meeting_id], |r| r.get(0))?
            .collect::<Result<_, _>>()?
    };
    for id in known.into_iter().filter(|id| !ids.contains(id)) {
        tx.execute("DELETE FROM summaries WHERE id = ?1", [id])?;
    }
    Ok(())
}

/// Every recorded summary of `meeting`, read from its file.
pub(crate) fn load_all(
    conn: &Connection,
    meeting: &MeetingRecord,
) -> Result<Vec<Summary>, StoreError> {
    let mut stmt = conn
        .prepare("SELECT id, file FROM summaries WHERE meeting_id = ?1 ORDER BY created_at, id")?;
    let rows: Vec<(String, String)> = stmt
        .query_map([&meeting.id], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut out = Vec::new();
    for (id, file) in rows {
        let path = meeting.folder.join(&file);
        match fs::read_to_string(&path).ok().and_then(|t| parse(&t)) {
            Some(mut summary) => {
                summary.meeting_id.clone_from(&meeting.id);
                out.push(summary);
            }
            None => {
                tracing::warn!(%id, file = %path.display(), "summary file missing or unreadable")
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_round_trips() {
        let summary = Summary {
            id: "0193-x".into(),
            meeting_id: "m".into(),
            title: Some("Weekly \"sync\": notes".into()),
            author_model: None,
            created_at: "2026-10-26T13:00:00Z".into(),
            markdown: "# Notes\n\n---\n\n- a\n".into(),
        };

        let text = render(&summary);

        assert!(text.starts_with("---\nid: \"0193-x\"\n"));
        assert_eq!(parse(&text).unwrap(), summary);
    }

    #[test]
    fn rejects_files_without_front_matter() {
        assert!(parse("# Just markdown").is_none());
        assert!(
            parse("---\nid: \"x\"\n---\nbody").is_none(),
            "missing created_at"
        );
    }
}
