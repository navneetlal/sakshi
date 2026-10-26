use clap::Subcommand;
use sakshi_core::DataDir;
use sakshi_store::{MeetingQuery, Store};

#[derive(Debug, Subcommand)]
pub enum MeetingsCommand {
    /// List processed meetings, most recent first.
    List {
        /// Only meetings whose title or transcript contains this text.
        #[arg(long)]
        query: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: u32,
    },
}

pub fn run(data: &DataDir, cmd: MeetingsCommand) -> anyhow::Result<()> {
    match cmd {
        MeetingsCommand::List { query, limit } => {
            let store = Store::open_read_only(&data.db_path())?;
            let meetings = store.meetings(&MeetingQuery {
                text: query,
                limit,
                ..MeetingQuery::default()
            })?;
            if meetings.is_empty() {
                println!("no meetings");
                return Ok(());
            }
            for m in meetings {
                let when = m.started_at.as_deref().unwrap_or(&m.indexed_at);
                let minutes = m.duration_ms as f64 / 60_000.0;
                println!(
                    "{}  {when}  {minutes:5.1} min  {} speakers  {}",
                    m.id,
                    m.speakers.len(),
                    m.title
                );
                println!("    {}", m.folder.display());
            }
            Ok(())
        }
    }
}
