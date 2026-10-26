//! Launches the `sakshi-mcp` binary the way an MCP client does: as a child
//! process speaking MCP over stdio.

use rmcp::model::CallToolRequestParams;
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use rmcp::{ServiceExt, object};
use sakshi_core::{DataDir, MeetingDir, Segment, Speaker, Track, Transcript, write_json_atomic};
use sakshi_store::Store;

#[tokio::test(flavor = "multi_thread")]
async fn serves_meetings_over_stdio() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::new(root.path().join("data"));
    let meeting = MeetingDir::new(root.path().join("standup"));
    write_json_atomic(
        &meeting.transcript_json(),
        &Transcript {
            schema_version: 1,
            duration_ms: 3000,
            languages: vec![],
            segments: vec![Segment {
                id: 0,
                start_ms: 0,
                end_ms: 2000,
                track: Track::System,
                speaker: Speaker::Remote(1),
                text: "hello from the other side".into(),
                words: vec![],
            }],
        },
    )
    .unwrap();
    let id = Store::open(&data.db_path())
        .unwrap()
        .index_meeting(&meeting)
        .unwrap()
        .id;

    let command = tokio::process::Command::new(env!("CARGO_BIN_EXE_sakshi-mcp")).configure(|cmd| {
        cmd.arg("--data-dir").arg(data.root());
    });
    let client = ().serve(TokioChildProcess::new(command).unwrap()).await.unwrap();

    let info = client.peer_info().unwrap();
    assert_eq!(info.server_info.as_ref().unwrap().name, "sakshi");
    assert_eq!(client.list_all_tools().await.unwrap().len(), 8);

    let result = client
        .call_tool(
            CallToolRequestParams::new("search_transcripts")
                .with_arguments(object!({"query": "other side"})),
        )
        .await
        .unwrap();
    let hits = result.structured_content.unwrap();
    assert_eq!(hits["results"][0]["meeting_id"], id.as_str());

    client.cancel().await.unwrap();
}
