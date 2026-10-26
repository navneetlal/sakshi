use std::path::Path;

use sakshi_core::{
    Frame, MeetingDir, MeetingInfo, Segment, Speaker, Track, Transcript, Word, read_json,
    write_json_atomic,
};
use sakshi_store::{MeetingQuery, Store, StoreError, SummaryWriter, TimeRange};

fn segment(id: u32, start_ms: u64, track: Track, speaker: Speaker, text: &str) -> Segment {
    let words: Vec<Word> = text
        .split_whitespace()
        .enumerate()
        .map(|(i, t)| Word {
            start_ms: start_ms + i as u64 * 300,
            end_ms: start_ms + i as u64 * 300 + 250,
            text: t.into(),
            p: 1.0,
        })
        .collect();
    Segment {
        id,
        start_ms,
        end_ms: words.last().map_or(start_ms, |w| w.end_ms),
        track,
        speaker,
        text: text.into(),
        words,
    }
}

/// A processed meeting folder.
fn meeting(
    dir: &Path,
    title: Option<&str>,
    started_at: Option<&str>,
    segments: Vec<Segment>,
) -> MeetingDir {
    let m = MeetingDir::new(dir);
    write_json_atomic(
        &m.transcript_json(),
        &Transcript {
            schema_version: 1,
            duration_ms: 60_000,
            languages: vec!["en-US".into()],
            segments,
        },
    )
    .unwrap();
    let frames: Vec<Frame> = [0u64, 20_000, 40_000]
        .iter()
        .map(|&at_ms| Frame {
            at_ms,
            file: format!("frames/{at_ms}.jpg"),
            width: 640,
            height: 360,
        })
        .collect();
    write_json_atomic(&m.frames_json(), &frames).unwrap();
    if title.is_some() || started_at.is_some() {
        let info = MeetingInfo {
            title: title.map(str::to_owned),
            started_at: started_at.map(str::to_owned),
            ..MeetingInfo::default()
        };
        write_json_atomic(&m.info_json(), &info).unwrap();
    }
    m
}

fn english() -> Vec<Segment> {
    vec![
        segment(
            0,
            1000,
            Track::System,
            Speaker::Remote(1),
            "From a plain text file",
        ),
        segment(
            1,
            5000,
            Track::Mic,
            Speaker::You,
            "I guess we just see what Steve says",
        ),
        segment(
            2,
            21_000,
            Track::System,
            Speaker::Remote(2),
            "Organize it by time, not by speaker",
        ),
        segment(
            3,
            41_000,
            Track::System,
            Speaker::Remote(1),
            "see you on Thursday",
        ),
    ]
}

fn hindi() -> Vec<Segment> {
    vec![
        segment(
            0,
            1280,
            Track::System,
            Speaker::Remote(1),
            "इसे मंगलवार को काटना तय किया गया था",
        ),
        segment(
            1,
            13_440,
            Track::System,
            Speaker::Remote(2),
            "इतालवी भाषा में उच्चारण करना आसान है",
        ),
    ]
}

struct Fixture {
    root: tempfile::TempDir,
    store: Store,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(&root.path().join("data/sakshi.db")).unwrap();
        Self { root, store }
    }

    fn db(&self) -> std::path::PathBuf {
        self.root.path().join("data/sakshi.db")
    }

    fn dir(&self, name: &str) -> std::path::PathBuf {
        let dir = self.root.path().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}

#[test]
fn indexes_a_meeting_and_assigns_a_stable_id() {
    let mut f = Fixture::new();
    let m = meeting(
        &f.dir("standup"),
        None,
        Some("2026-10-20T09:30:00+05:30"),
        english(),
    );

    let record = f.store.index_meeting(&m).unwrap();

    assert!(uuid::Uuid::parse_str(&record.id).is_ok());
    assert_eq!(record.title, "standup");
    assert_eq!(record.started_at.as_deref(), Some("2026-10-20T04:00:00Z"));
    assert_eq!(record.duration_ms, 60_000);
    assert_eq!(record.speakers, ["Speaker 1", "You", "Speaker 2"]);
    assert_eq!((record.segment_count, record.frame_count), (4, 3));
    let info: MeetingInfo = read_json(&m.info_json()).unwrap();
    assert_eq!(info.id.as_deref(), Some(record.id.as_str()));

    // Re-indexing keeps the id and replaces the content.
    let again = f.store.index_meeting(&m).unwrap();
    assert_eq!(again.id, record.id);
    assert_eq!(
        f.store
            .meetings(&MeetingQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        f.store
            .segments(&record.id, TimeRange::ALL, None, 100)
            .unwrap()
            .len(),
        4
    );
}

#[test]
fn a_moved_folder_keeps_its_id() {
    let mut f = Fixture::new();
    let old = f.dir("old");
    let id = f
        .store
        .index_meeting(&meeting(&old, None, None, english()))
        .unwrap()
        .id;

    let new = f.root.path().join("new");
    std::fs::rename(&old, &new).unwrap();
    let record = f.store.index_meeting(&MeetingDir::new(&new)).unwrap();

    assert_eq!(record.id, id);
    assert_eq!(record.folder, std::path::absolute(&new).unwrap());
    assert_eq!(
        f.store
            .meetings(&MeetingQuery {
                limit: 10,
                ..Default::default()
            })
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn unprocessed_folders_are_rejected() {
    let mut f = Fixture::new();
    let err = f
        .store
        .index_meeting(&MeetingDir::new(f.dir("raw")))
        .unwrap_err();
    assert!(matches!(err, StoreError::NotProcessed { .. }), "{err}");
}

#[test]
fn lists_meetings_by_time_and_text() {
    let mut f = Fixture::new();
    let a = f
        .store
        .index_meeting(&meeting(
            &f.dir("a"),
            Some("Design review"),
            Some("2026-10-01T10:00:00Z"),
            english(),
        ))
        .unwrap();
    let b = f
        .store
        .index_meeting(&meeting(
            &f.dir("b"),
            Some("हिंदी बैठक"),
            Some("2026-10-15T10:00:00Z"),
            hindi(),
        ))
        .unwrap();
    let q = |since: Option<&str>, until: Option<&str>, text: Option<&str>| MeetingQuery {
        since: since.map(|s| s.parse().unwrap()),
        until: until.map(|s| s.parse().unwrap()),
        text: text.map(str::to_owned),
        limit: 10,
    };
    let ids = |query: MeetingQuery| -> Vec<String> {
        f.store
            .meetings(&query)
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect()
    };

    let (a, b) = (vec![a.id], vec![b.id]);
    assert_eq!(ids(q(None, None, None)), [b.clone(), a.clone()].concat());
    assert_eq!(ids(q(Some("2026-10-10T00:00:00Z"), None, None)), b);
    assert_eq!(ids(q(None, Some("2026-10-10T00:00:00Z"), None)), a);
    assert_eq!(ids(q(None, None, Some("design"))), a);
    assert_eq!(ids(q(None, None, Some("thursday"))), a);
    // Two characters are too short for trigrams; a scan finds them.
    assert_eq!(ids(q(None, None, Some("by"))), a);
    assert_eq!(ids(q(None, None, Some("मंगलवार"))), b);
    assert!(ids(q(None, None, Some("nothing like this"))).is_empty());
    assert_eq!(
        f.store
            .meetings(&MeetingQuery {
                limit: 1,
                ..Default::default()
            })
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn pages_segments_and_finds_frames() {
    let mut f = Fixture::new();
    let id = f
        .store
        .index_meeting(&meeting(&f.dir("m"), None, None, english()))
        .unwrap()
        .id;

    let page1 = f.store.segments(&id, TimeRange::ALL, None, 2).unwrap();
    let page2 = f
        .store
        .segments(&id, TimeRange::ALL, Some(page1[1].id), 2)
        .unwrap();
    assert_eq!(page1.iter().map(|s| s.id).collect::<Vec<_>>(), [0, 1]);
    assert_eq!(page2.iter().map(|s| s.id).collect::<Vec<_>>(), [2, 3]);
    assert_eq!(page1[1].speaker, Speaker::You);
    assert_eq!(page1[1].words.len(), 8);

    let window = TimeRange {
        start_ms: 6000,
        end_ms: 30_000,
    };
    let ids: Vec<u32> = f
        .store
        .segments(&id, window, None, 10)
        .unwrap()
        .iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(ids, [1, 2], "segment 1 runs past 6 s, so it overlaps");

    let frames = f
        .store
        .frames(
            &id,
            TimeRange {
                start_ms: 10_000,
                end_ms: 45_000,
            },
        )
        .unwrap();
    assert_eq!(
        frames.iter().map(|f| f.at_ms).collect::<Vec<_>>(),
        [20_000, 40_000]
    );
    assert_eq!(
        f.store.frame_at(&id, 39_999).unwrap().unwrap().at_ms,
        20_000
    );
    assert_eq!(
        f.store.frame_at(&id, 40_000).unwrap().unwrap().at_ms,
        40_000
    );
    assert_eq!(f.store.frame_at("missing", 0).unwrap(), None);
}

#[test]
fn searches_english_and_hindi() {
    let mut f = Fixture::new();
    let en = f
        .store
        .index_meeting(&meeting(&f.dir("en"), None, None, english()))
        .unwrap()
        .id;
    let hi = f
        .store
        .index_meeting(&meeting(&f.dir("hi"), None, None, hindi()))
        .unwrap()
        .id;

    let hits = f.store.search("STEVE says", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(
        (hits[0].meeting_id.as_str(), hits[0].segment_id),
        (en.as_str(), 1)
    );
    assert_eq!(hits[0].speaker, Speaker::You);

    // Substrings inside Devanagari words match.
    let hits = f.store.search("उच्चार", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].meeting_id, hi);

    // Two characters are too short for trigrams and fall back to a scan.
    assert_eq!(f.store.search("by", 10).unwrap().len(), 1);
    // Quotes and FTS syntax are matched literally.
    assert!(f.store.search("\"plain\" OR text", 10).unwrap().is_empty());
    assert!(f.store.search("   ", 10).unwrap().is_empty());
}

#[test]
fn saves_summaries_and_recovers_them_from_files() {
    let mut f = Fixture::new();
    let dir = f.dir("m");
    let id = f
        .store
        .index_meeting(&meeting(&dir, None, None, english()))
        .unwrap()
        .id;
    let mut writer = SummaryWriter::open(&f.db()).unwrap();

    let saved = writer
        .save(
            &id,
            "# Notes\n\n- ship it\n",
            Some("Weekly notes"),
            Some("claude-opus-5-5"),
        )
        .unwrap();

    let path = dir.join(format!("summaries/{}.md", saved.id));
    assert!(path.is_file());
    let listed = f.store.summaries(&id).unwrap();
    assert_eq!(listed, std::slice::from_ref(&saved));
    assert_eq!(listed[0].markdown, "# Notes\n\n- ship it\n");

    // A fresh database rebuilt from the folder finds the summary again.
    drop(writer);
    let mut rebuilt = Store::open(&f.root.path().join("other.db")).unwrap();
    rebuilt.index_meeting(&MeetingDir::new(&dir)).unwrap();
    assert_eq!(rebuilt.summaries(&id).unwrap(), [saved]);

    // Deleting the file removes the summary on the next index.
    std::fs::remove_file(&path).unwrap();
    rebuilt.index_meeting(&MeetingDir::new(&dir)).unwrap();
    assert!(rebuilt.summaries(&id).unwrap().is_empty());
}

#[test]
fn summary_input_is_validated() {
    let mut f = Fixture::new();
    let id = f
        .store
        .index_meeting(&meeting(&f.dir("m"), None, None, english()))
        .unwrap()
        .id;
    let mut writer = SummaryWriter::open(&f.db()).unwrap();

    for (markdown, title) in [("  \n", None), ("ok", Some("two\nlines"))] {
        assert!(matches!(
            writer.save(&id, markdown, title, None),
            Err(StoreError::InvalidSummary(_))
        ));
    }
    let big = "x".repeat(300 * 1024);
    assert!(matches!(
        writer.save(&id, &big, None, None),
        Err(StoreError::InvalidSummary(_))
    ));
    assert!(matches!(
        writer.save("0193f5c0-0000-7000-8000-000000000000", "ok", None, None),
        Err(StoreError::UnknownMeeting(_))
    ));
}

#[test]
fn read_only_handles_cannot_write_or_create() {
    let mut f = Fixture::new();
    let m = meeting(&f.dir("m"), None, None, english());
    f.store.index_meeting(&m).unwrap();

    let mut ro = Store::open_read_only(&f.db()).unwrap();
    assert_eq!(
        ro.meetings(&MeetingQuery {
            limit: 5,
            ..Default::default()
        })
        .unwrap()
        .len(),
        1
    );
    assert!(matches!(ro.index_meeting(&m), Err(StoreError::Sqlite(_))));

    let missing = f.root.path().join("none.db");
    assert!(matches!(
        Store::open_read_only(&missing),
        Err(StoreError::NoDatabase { .. })
    ));
    assert!(matches!(
        SummaryWriter::open(&missing),
        Err(StoreError::NoDatabase { .. })
    ));
    assert!(!missing.exists());
}
