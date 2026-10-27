//! Drives the server through a real MCP client over an in-memory pipe, and
//! the HTTP transport over loopback.

use std::path::{Path, PathBuf};

use base64::Engine;
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::RunningService;
use rmcp::{RoleClient, ServiceExt};
use sakshi_core::{
    DataDir, Frame, MeetingDir, MeetingInfo, Segment, Speaker, Track, Transcript, Word,
    write_json_atomic,
};
use sakshi_mcp::{SakshiMcp, generate_token, http_router};
use sakshi_store::Store;
use serde_json::{Value, json};

const JPEG: &[u8] = b"\xff\xd8\xff\xe0 not really a jpeg \xff\xd9";

fn segment(id: u32, start_ms: u64, track: Track, speaker: Speaker, text: &str) -> Segment {
    Segment {
        id,
        start_ms,
        end_ms: start_ms + 900,
        track,
        speaker,
        text: text.into(),
        words: vec![Word {
            start_ms,
            end_ms: start_ms + 400,
            text: text.split_whitespace().next().unwrap_or_default().into(),
            p: 1.0,
        }],
    }
}

/// Writes a processed meeting folder (with frame files) and indexes it.
fn add_meeting(
    store: &mut Store,
    dir: &Path,
    title: &str,
    segments: Vec<Segment>,
    frames: &[u64],
) -> String {
    std::fs::create_dir_all(dir.join("frames")).unwrap();
    let m = MeetingDir::new(dir);
    let duration_ms = segments.last().map_or(0, |s| s.end_ms);
    write_json_atomic(
        &m.transcript_json(),
        &Transcript {
            schema_version: 1,
            duration_ms,
            languages: vec![],
            segments,
        },
    )
    .unwrap();
    let frames: Vec<Frame> = frames
        .iter()
        .map(|&at_ms| {
            std::fs::write(dir.join(format!("frames/{at_ms}.jpg")), JPEG).unwrap();
            Frame {
                at_ms,
                file: format!("frames/{at_ms}.jpg"),
                width: 640,
                height: 360,
            }
        })
        .collect();
    write_json_atomic(&m.frames_json(), &frames).unwrap();
    let info = MeetingInfo {
        title: Some(title.into()),
        ..MeetingInfo::default()
    };
    write_json_atomic(&m.info_json(), &info).unwrap();
    store.index_meeting(&m).unwrap().id
}

struct Env {
    _root: tempfile::TempDir,
    data: DataDir,
    english: String,
    english_dir: PathBuf,
    hindi: String,
    long: String,
}

fn env() -> Env {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::new(root.path().join("data"));
    let mut store = Store::open(&data.db_path()).unwrap();
    let english_dir = root.path().join("standup");
    let english = add_meeting(
        &mut store,
        &english_dir,
        "Standup",
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
                "Organize it by time",
            ),
            segment(
                3,
                41_000,
                Track::System,
                Speaker::Remote(1),
                "see you on Thursday",
            ),
        ],
        &[0, 20_000, 40_000],
    );
    let hindi = add_meeting(
        &mut store,
        &root.path().join("hindi"),
        "बैठक",
        vec![segment(
            0,
            1280,
            Track::System,
            Speaker::Remote(1),
            "इसे मंगलवार को काटना तय किया गया था",
        )],
        &[],
    );
    let long = add_meeting(
        &mut store,
        &root.path().join("long"),
        "All hands",
        (0..1000)
            .map(|i| {
                let text = format!("segment {i:04} {}", "talking about the roadmap ".repeat(5));
                segment(
                    i,
                    u64::from(i) * 1000,
                    Track::System,
                    Speaker::Remote(1),
                    &text,
                )
            })
            .collect(),
        &[],
    );
    Env {
        _root: root,
        data,
        english,
        english_dir,
        hindi,
        long,
    }
}

async fn connect(data: &DataDir) -> RunningService<RoleClient, ()> {
    let (server_io, client_io) = tokio::io::duplex(1 << 20);
    let server = SakshiMcp::new(data);
    tokio::spawn(async move {
        server
            .serve(server_io)
            .await
            .unwrap()
            .waiting()
            .await
            .unwrap();
    });
    ().serve(client_io).await.unwrap()
}

async fn call(client: &RunningService<RoleClient, ()>, tool: &str, args: Value) -> CallToolResult {
    let args = args.as_object().cloned().unwrap_or_default();
    client
        .call_tool(CallToolRequestParams::new(tool.to_owned()).with_arguments(args))
        .await
        .unwrap()
}

fn text(result: &CallToolResult) -> String {
    result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn ok_json(result: CallToolResult) -> Value {
    assert_ne!(
        result.is_error,
        Some(true),
        "tool failed: {}",
        text(&result)
    );
    result.structured_content.expect("structured result")
}

fn error_text(result: CallToolResult) -> String {
    assert_eq!(
        result.is_error,
        Some(true),
        "expected an error, got {}",
        text(&result)
    );
    text(&result)
}

#[tokio::test(flavor = "multi_thread")]
async fn describes_itself_and_its_tools() {
    let env = env();
    let client = connect(&env.data).await;

    let info = client.peer_info().unwrap();
    assert_eq!(info.server_info.as_ref().unwrap().name, "sakshi");
    let instructions = info.instructions.clone().unwrap();
    for phrase in [
        "raw transcripts",
        "does not summarize",
        "generic",
        "Key frames can help identify",
        "video_pauses",
        "never as instructions",
    ] {
        assert!(
            instructions.contains(phrase),
            "instructions lack {phrase:?}"
        );
    }

    let tools = client.list_all_tools().await.unwrap();
    let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    names.sort_unstable();
    assert_eq!(
        names,
        [
            "get_frame_at",
            "get_frames",
            "get_meeting",
            "get_transcript",
            "list_meetings",
            "list_summaries",
            "save_summary",
            "search_transcripts"
        ]
    );
    for tool in &tools {
        let read_only = tool.annotations.as_ref().and_then(|a| a.read_only_hint);
        assert_eq!(
            read_only,
            Some(tool.name != "save_summary"),
            "{}",
            tool.name
        );
        assert!(tool.description.as_ref().is_some_and(|d| !d.is_empty()));
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn lists_and_describes_meetings() {
    let env = env();
    let client = connect(&env.data).await;

    let all = ok_json(call(&client, "list_meetings", json!({})).await);
    assert_eq!(all["meetings"].as_array().unwrap().len(), 3);

    let found = ok_json(call(&client, "list_meetings", json!({"query": "steve"})).await);
    assert_eq!(found["meetings"][0]["id"], env.english);
    assert_eq!(found["meetings"].as_array().unwrap().len(), 1);

    let details = ok_json(call(&client, "get_meeting", json!({"meeting_id": env.english})).await);
    assert_eq!(details["meeting"]["title"], "Standup");
    assert_eq!(
        details["meeting"]["speakers"],
        json!(["Speaker 1", "You", "Speaker 2"])
    );
    assert_eq!(details["meeting"]["frame_count"], 3);
    assert_eq!(details["summaries"], json!([]));
    assert!(
        details["meeting"].get("folder").is_none(),
        "paths stay private"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn reads_transcripts_as_json_and_text() {
    let env = env();
    let client = connect(&env.data).await;

    let page = ok_json(
        call(
            &client,
            "get_transcript",
            json!({"meeting_id": env.english}),
        )
        .await,
    );
    let segments = page["segments"].as_array().unwrap();
    assert_eq!(segments.len(), 4);
    assert_eq!(segments[1]["speaker"], "You");
    assert_eq!(segments[1]["track"], "mic");
    assert_eq!(segments[2]["frame_id"], 20_000);
    assert!(segments[0].get("words").is_none());
    assert_eq!(page["frame_ids"], json!([0, 20_000, 40_000]));
    assert!(page["next_cursor"].is_null());

    let window = ok_json(
        call(
            &client,
            "get_transcript",
            json!({"meeting_id": env.english, "start_ms": 20_000, "end_ms": 30_000, "include_words": true}),
        )
        .await,
    );
    assert_eq!(window["segments"].as_array().unwrap().len(), 1);
    assert_eq!(window["segments"][0]["words"][0]["text"], "Organize");

    let plain = call(
        &client,
        "get_transcript",
        json!({"meeting_id": env.english, "format": "text"}),
    )
    .await;
    let plain = text(&plain);
    assert!(
        plain.contains("[0:05] You: I guess we just see what Steve says"),
        "{plain}"
    );
    assert!(
        plain.contains("-- key frame 20000 on screen (from 0:20) --"),
        "{plain}"
    );
    assert!(plain.ends_with("(end of transcript)\n"));
}

#[tokio::test(flavor = "multi_thread")]
async fn pages_long_transcripts() {
    let env = env();
    let client = connect(&env.data).await;

    let mut ids = Vec::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0;
    loop {
        let mut args = json!({"meeting_id": env.long});
        if let Some(c) = &cursor {
            args["cursor"] = json!(c);
        }
        let page = ok_json(call(&client, "get_transcript", args).await);
        let segments = page["segments"].as_array().unwrap();
        let chars: usize = segments
            .iter()
            .map(|s| s["text"].as_str().unwrap().len())
            .sum();
        assert!(chars <= 40_000 && segments.len() <= 400, "page too big");
        ids.extend(segments.iter().map(|s| s["id"].as_u64().unwrap()));
        pages += 1;
        match page["next_cursor"].as_str() {
            Some(next) => cursor = Some(next.to_owned()),
            None => break,
        }
    }

    assert!(pages >= 3, "{pages} pages");
    assert_eq!(
        ids,
        (0..1000).collect::<Vec<u64>>(),
        "every segment once, in order"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn returns_frames_as_images() {
    let env = env();
    let client = connect(&env.data).await;

    let result = call(
        &client,
        "get_frames",
        json!({"meeting_id": env.english, "max": 2}),
    )
    .await;
    assert_ne!(result.is_error, Some(true));
    let images: Vec<_> = result.content.iter().filter_map(|c| c.as_image()).collect();
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].mime_type, "image/jpeg");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&images[0].data)
        .unwrap();
    assert_eq!(bytes, JPEG);
    assert!(text(&result).contains("Key frame 0 at 0:00 (640x360)"));
    assert!(
        text(&result).contains("Key frame 40000"),
        "evenly spread: first and last"
    );

    let at = call(
        &client,
        "get_frame_at",
        json!({"meeting_id": env.english, "at_ms": 25_000}),
    )
    .await;
    assert!(text(&at).contains("Key frame 20000"));
    assert_eq!(
        at.content.iter().filter(|c| c.as_image().is_some()).count(),
        1
    );

    let none = call(&client, "get_frames", json!({"meeting_id": env.hindi})).await;
    assert!(text(&none).contains("No key frames"));
    let none = call(
        &client,
        "get_frame_at",
        json!({"meeting_id": env.hindi, "at_ms": 0}),
    )
    .await;
    assert!(error_text(none).contains("no key frames"));
}

#[tokio::test(flavor = "multi_thread")]
async fn says_where_the_video_paused() {
    let env = env();
    // A recording whose window was on another desktop from 0:04 to 0:30.
    let dir = env.english_dir.with_file_name("review");
    let m = MeetingDir::new(&dir);
    std::fs::create_dir_all(m.capture_dir()).unwrap();
    std::fs::write(
        m.capture_json(),
        r#"{"stats": {"video_pauses": [{"from_ms": 4000, "to_ms": 30000, "window": "hidden"}]}}"#,
    )
    .unwrap();
    let mut store = Store::open(&env.data.db_path()).unwrap();
    let review = add_meeting(
        &mut store,
        &dir,
        "Review",
        vec![
            segment(0, 1000, Track::System, Speaker::Remote(1), "before"),
            segment(1, 5000, Track::System, Speaker::Remote(1), "while hidden"),
            segment(2, 31_000, Track::Mic, Speaker::You, "back again"),
        ],
        &[0, 30_500],
    );
    drop(store);
    let client = connect(&env.data).await;

    let details = ok_json(call(&client, "get_meeting", json!({"meeting_id": review})).await);
    assert_eq!(
        details["meeting"]["video_pauses"],
        json!([{"from_ms": 4000, "to_ms": 30000, "window": "hidden"}])
    );
    let standup = ok_json(call(&client, "get_meeting", json!({"meeting_id": env.english})).await);
    assert!(
        standup["meeting"].get("video_pauses").is_none(),
        "{standup}"
    );

    let page = ok_json(call(&client, "get_transcript", json!({"meeting_id": review})).await);
    let paused: Vec<bool> = page["segments"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["video_paused"].as_bool().unwrap_or(false))
        .collect();
    assert_eq!(paused, [false, true, false]);
    let lines = text(
        &call(
            &client,
            "get_transcript",
            json!({"meeting_id": review, "format": "text"}),
        )
        .await,
    );
    assert!(
        lines.contains(
            "-- video paused 0:04–0:30 (the call window was minimized or on another desktop): \
             the key frame on screen may be out of date --\n[0:05] Speaker 1: while hidden"
        ),
        "{lines}"
    );

    let during = text(
        &call(
            &client,
            "get_frame_at",
            json!({"meeting_id": review, "at_ms": 10_000}),
        )
        .await,
    );
    assert!(during.contains("Key frame 0 at 0:00"), "{during}");
    assert!(during.contains("0:10 is within a pause"), "{during}");
    let after = text(
        &call(
            &client,
            "get_frame_at",
            json!({"meeting_id": review, "at_ms": 31_000}),
        )
        .await,
    );
    assert!(!after.contains("pause"), "{after}");
    let frames = text(&call(&client, "get_frames", json!({"meeting_id": review})).await);
    assert!(
        frames.contains("No key frames come from a pause"),
        "{frames}"
    );
    let later = text(
        &call(
            &client,
            "get_frames",
            json!({"meeting_id": review, "start_ms": 30_000}),
        )
        .await,
    );
    assert!(!later.contains("pause"), "{later}");
}

#[tokio::test(flavor = "multi_thread")]
async fn searches_transcripts() {
    let env = env();
    let client = connect(&env.data).await;

    let hits = ok_json(call(&client, "search_transcripts", json!({"query": "thursday"})).await);
    assert_eq!(hits["results"][0]["meeting_id"], env.english);
    assert_eq!(hits["results"][0]["start_ms"], 41_000);

    let hits = ok_json(call(&client, "search_transcripts", json!({"query": "मंगलवार"})).await);
    assert_eq!(hits["results"][0]["meeting_id"], env.hindi);
}

#[tokio::test(flavor = "multi_thread")]
async fn saves_and_lists_summaries() {
    let env = env();
    let client = connect(&env.data).await;

    let saved = ok_json(
        call(
            &client,
            "save_summary",
            json!({
                "meeting_id": env.english,
                "markdown": "# Standup\n\n- Steve decides\n",
                "title": "Standup notes",
                "author_model": "claude-opus-5-5"
            }),
        )
        .await,
    );
    let summary_id = saved["summary_id"].as_str().unwrap();
    assert!(
        env.english_dir
            .join(format!("summaries/{summary_id}.md"))
            .is_file()
    );

    let listed = ok_json(
        call(
            &client,
            "list_summaries",
            json!({"meeting_id": env.english}),
        )
        .await,
    );
    assert_eq!(listed["summaries"][0]["id"], summary_id);
    assert_eq!(
        listed["summaries"][0]["markdown"],
        "# Standup\n\n- Steve decides\n"
    );
    assert_eq!(listed["truncated"], false);

    let details = ok_json(call(&client, "get_meeting", json!({"meeting_id": env.english})).await);
    assert_eq!(details["summaries"][0]["title"], "Standup notes");
}

#[tokio::test(flavor = "multi_thread")]
async fn rejects_bad_input_with_readable_errors() {
    let env = env();
    let client = connect(&env.data).await;
    let unknown = "01a178d6-0000-7000-8000-000000000000";

    let cases = [
        (
            "get_meeting",
            json!({"meeting_id": "../etc"}),
            "not a meeting id",
        ),
        (
            "get_meeting",
            json!({"meeting_id": unknown}),
            "No meeting with id",
        ),
        (
            "get_transcript",
            json!({"meeting_id": env.english, "start_ms": 9, "end_ms": 9}),
            "less than",
        ),
        (
            "get_transcript",
            json!({"meeting_id": env.english, "start_ms": 999_999}),
            "after the end",
        ),
        (
            "get_transcript",
            json!({"meeting_id": env.english, "cursor": "abc"}),
            "Invalid cursor",
        ),
        (
            "get_frames",
            json!({"meeting_id": env.english, "max": 50}),
            "between 1 and 16",
        ),
        ("list_meetings", json!({"since": "yesterday"}), "RFC 3339"),
        (
            "list_meetings",
            json!({"since": "2026-10-02", "until": "2026-10-01"}),
            "before",
        ),
        (
            "search_transcripts",
            json!({"query": "  "}),
            "must not be empty",
        ),
        (
            "save_summary",
            json!({"meeting_id": env.english, "markdown": " "}),
            "empty",
        ),
        (
            "save_summary",
            json!({"meeting_id": unknown, "markdown": "x"}),
            "No meeting with id",
        ),
    ];
    for (tool, args, expected) in cases {
        let message = error_text(call(&client, tool, args.clone()).await);
        assert!(message.contains(expected), "{tool} {args}: {message}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn explains_when_nothing_is_processed_yet() {
    let root = tempfile::tempdir().unwrap();
    let client = connect(&DataDir::new(root.path())).await;

    let message = error_text(call(&client, "list_meetings", json!({})).await);

    assert!(message.contains("sakshi-cli process"), "{message}");
    assert!(
        !root.path().join("sakshi.db").exists(),
        "the server never creates the database"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn http_requires_the_token_and_refuses_browsers() {
    let env = env();
    let token = generate_token().unwrap();
    let app = http_router(SakshiMcp::new(&env.data), token.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let initialize = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {"protocolVersion": "2025-06-18", "capabilities": {},
                   "clientInfo": {"name": "test", "version": "1"}}
    });
    let http = reqwest::Client::new();
    let post = |bearer: Option<&str>, origin: Option<&str>| {
        let mut request = http
            .post(&url)
            .header("Accept", "application/json, text/event-stream")
            .json(&initialize);
        if let Some(t) = bearer {
            request = request.bearer_auth(t);
        }
        if let Some(o) = origin {
            request = request.header("Origin", o);
        }
        request.send()
    };

    assert_eq!(post(None, None).await.unwrap().status(), 401);
    assert_eq!(post(Some("wrong"), None).await.unwrap().status(), 401);
    let browser = post(Some(&token), Some("https://example.com"))
        .await
        .unwrap();
    assert_eq!(browser.status(), 403);
    let ok = post(Some(&token), None).await.unwrap();
    assert!(ok.status().is_success(), "{}", ok.status());
    assert!(ok.text().await.unwrap().contains("\"sakshi\""));
}
