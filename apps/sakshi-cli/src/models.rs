use std::io::{IsTerminal, Write};
use std::sync::{Arc, Mutex};

use anyhow::{Context, bail};
use clap::Subcommand;
use sakshi_core::DataDir;
use sakshi_models::{Hub, ModelEntry, ModelStore, Role};

#[derive(Debug, Subcommand)]
pub enum ModelsCommand {
    /// Show every known model and whether it is downloaded.
    List,
    /// Download models. With no ids, downloads the default ASR and
    /// diarization models.
    Pull {
        ids: Vec<String>,
        /// Accept each model's license without asking.
        #[arg(long)]
        yes: bool,
    },
    /// Add a GGUF model from any Hugging Face repository, then download it.
    /// The file must be one the nemo-speech runtime can run.
    Add {
        /// Repository id, e.g. `owner/name`.
        repo: String,
        /// Path of the .gguf file inside the repository.
        file: String,
        /// Branch, tag, or commit. The model is pinned to the commit this
        /// resolves to now.
        #[arg(long)]
        revision: Option<String>,
        /// Accept the model's license without asking.
        #[arg(long)]
        yes: bool,
    },
}

pub fn run(data: &DataDir, cmd: ModelsCommand) -> anyhow::Result<()> {
    let store = ModelStore::open(data)?;
    match cmd {
        ModelsCommand::List => list(&store),
        ModelsCommand::Pull { ids, yes } => {
            let entries = if ids.is_empty() {
                vec![
                    store.default_for(Role::Asr)?,
                    store.default_for(Role::Diarization)?,
                ]
            } else {
                ids.iter()
                    .map(|id| store.find(id))
                    .collect::<Result<_, _>>()?
            };
            let hub = Hub::new()?;
            for entry in &entries {
                pull(&hub, &store, entry, yes)?;
            }
            Ok(())
        }
        ModelsCommand::Add {
            repo,
            file,
            revision,
            yes,
        } => {
            let hub = Hub::new()?;
            let custom = hub.inspect_custom(&repo, &file, revision.as_deref())?;
            println!(
                "{} is a GGUF {} model ({}), pinned to commit {}.",
                custom.entry.filename,
                custom.entry.role,
                custom.architecture,
                custom.entry.revision
            );
            store.add_custom(custom.entry.clone())?;
            pull(&hub, &store, &custom.entry, yes)
        }
    }
}

fn list(store: &ModelStore) -> anyhow::Result<()> {
    let entries = store.entries()?;
    let id_width = entries.iter().map(|m| m.id.len()).max().unwrap_or(2).max(2);
    println!(
        "{:id_width$}  {:13}  {:>9}  {:10}  LICENSE",
        "ID", "ROLE", "SIZE", "STATUS"
    );
    for entry in &entries {
        let status = match store.installed_path(entry)? {
            Some(_) => "downloaded",
            None => "-",
        };
        let role = if entry.default {
            format!("{} *", entry.role)
        } else {
            entry.role.to_string()
        };
        println!(
            "{:id_width$}  {role:13}  {:>9}  {status:10}  {}",
            entry.id,
            human_size(entry.size),
            entry.license
        );
    }
    println!(
        "\n* default for its role. Models are stored in {}",
        store.dir().display()
    );
    Ok(())
}

fn pull(hub: &Hub, store: &ModelStore, entry: &ModelEntry, yes: bool) -> anyhow::Result<()> {
    if let Some(path) = store.installed_path(entry)? {
        println!("{}: already downloaded ({})", entry.id, path.display());
        return Ok(());
    }
    println!("\n{} ({})", entry.name, human_size(entry.size));
    println!("  repository: {}", entry.repo_url());
    println!("  license:    {}", entry.license);
    println!("  terms:      {}", entry.license_url);
    let accepted = yes || confirm("Accept this license and download?")?;
    if !accepted {
        bail!("license not accepted; {} was not downloaded", entry.id);
    }

    // A live percentage on a terminal; one line per 10% when logging.
    let live = std::io::stderr().is_terminal();
    let step = if live { 1 } else { 10 };
    let last = Mutex::new(u64::MAX);
    let progress = Arc::new(move |done: u64, total: u64| {
        let pct = (done * 100).checked_div(total).unwrap_or(0) / step * step;
        let mut last = last.lock().expect("progress lock");
        if *last != pct {
            *last = pct;
            if live {
                eprint!("\r  downloading: {pct:3}%");
            } else {
                eprintln!("  downloading: {pct:3}%");
            }
        }
    });
    let path = hub.download(store, entry, true, Some(progress))?;
    if live {
        eprintln!("\r  downloading: done");
    }
    println!("  verified and saved to {}", path.display());
    Ok(())
}

fn confirm(question: &str) -> anyhow::Result<bool> {
    if !std::io::stdin().is_terminal() {
        bail!("{question} Re-run with --yes to accept the license non-interactively.");
    }
    print!("{question} [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .context("reading answer")?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "Yes"))
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_are_human_readable() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(107_012_128), "107.0 MB");
        assert_eq!(human_size(741_548_352), "741.5 MB");
        assert_eq!(human_size(1_178_100_960), "1.2 GB");
    }
}
