//! Names the user gives a meeting's speakers.
//!
//! Names are kept in the meeting's `meeting.json`, the source of truth, and
//! copied into the index. Transcripts keep their generic labels, so a name
//! can be changed or removed at any time.

use std::path::Path;

use rusqlite::{Connection, params};
use sakshi_core::{MeetingDir, MeetingInfo, SpeakerNames, read_json, write_json_atomic};

use crate::StoreError;
use crate::store::open_existing;

/// Names speakers, and nothing else: one of the two writes the MCP server
/// makes, besides saving summaries.
pub struct SpeakerNameWriter {
    conn: Connection,
}

impl std::fmt::Debug for SpeakerNameWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpeakerNameWriter").finish_non_exhaustive()
    }
}

impl SpeakerNameWriter {
    /// Opens an existing database, never creating one. One written by an
    /// older Sakshi is upgraded first, as for reading.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        Ok(Self {
            conn: open_existing(path)?,
        })
    }

    /// See [`save`].
    pub fn save(
        &mut self,
        meeting_id: &str,
        changes: &[(String, Option<String>)],
    ) -> Result<SpeakerNames, StoreError> {
        save(&self.conn, meeting_id, changes)
    }
}

/// Names (or, with `None` or a blank name, un-names) speakers of a meeting
/// by label, and returns all its names. Each label must be one of the
/// meeting's speakers. Writes `meeting.json` first, then the index.
pub(crate) fn save(
    conn: &Connection,
    meeting_id: &str,
    changes: &[(String, Option<String>)],
) -> Result<SpeakerNames, StoreError> {
    let (folder, speakers): (String, String) = conn
        .query_row(
            "SELECT folder, speakers FROM meetings WHERE id = ?1",
            [meeting_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => {
                StoreError::UnknownMeeting(meeting_id.to_owned())
            }
            e => e.into(),
        })?;
    let speakers: Vec<String> = serde_json::from_str(&speakers).unwrap_or_default();

    let dir = MeetingDir::new(folder);
    let mut info: MeetingInfo = if dir.info_json().exists() {
        read_json(&dir.info_json())?
    } else {
        MeetingInfo::default()
    };
    for (label, name) in changes {
        if !speakers.contains(label) {
            return Err(StoreError::UnknownSpeaker {
                label: label.clone(),
                speakers: speakers.join(", "),
            });
        }
        info.speaker_names.set(label, name.as_deref())?;
    }
    write_json_atomic(&dir.info_json(), &info)?;
    conn.execute(
        "UPDATE meetings SET speaker_names = ?1 WHERE id = ?2",
        params![
            serde_json::to_string(&info.speaker_names).expect("names serialize"),
            meeting_id
        ],
    )?;
    Ok(info.speaker_names)
}
