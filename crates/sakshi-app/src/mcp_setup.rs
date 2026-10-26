//! Connecting MCP clients to the `sakshi-mcp` server. Claude Desktop and
//! Cursor keep their servers in a JSON file that Sakshi can update (after
//! backing it up); for Claude Code and Codex it shows what to run or paste.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use sakshi_core::{DataDir, write_json_atomic};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::AppError;

/// The name Sakshi registers under in every client.
pub const SERVER_NAME: &str = "sakshi";

/// Environment variable that overrides where `sakshi-mcp` is.
pub const MCP_BIN_ENV: &str = "SAKSHI_MCP_BIN";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Client {
    ClaudeDesktop,
    ClaudeCode,
    Codex,
    Cursor,
}

impl Client {
    pub const ALL: [Self; 4] = [
        Self::ClaudeDesktop,
        Self::ClaudeCode,
        Self::Codex,
        Self::Cursor,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeDesktop => "claude-desktop",
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Cursor => "cursor",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::ClaudeDesktop => "Claude Desktop",
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::Cursor => "Cursor",
        }
    }

    /// Whether [`install`] can add Sakshi to this client's config file.
    pub fn can_write(self) -> bool {
        matches!(self, Self::ClaudeDesktop | Self::Cursor)
    }
}

impl fmt::Display for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Client {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|c| c.as_str() == s)
            .ok_or_else(|| {
                let names: Vec<_> = Self::ALL.iter().map(|c| c.as_str()).collect();
                format!("unknown client {s:?}; expected one of {}", names.join(", "))
            })
    }
}

/// How a client should start the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServerLaunch {
    pub command: PathBuf,
    pub args: Vec<String>,
}

impl ServerLaunch {
    /// Runs `command` against `data`. `--data-dir` is passed only when
    /// `data` isn't the folder the server would pick by itself, because
    /// clients don't pass on `SAKSHI_DATA_DIR`.
    pub fn new(command: PathBuf, data: &DataDir) -> Self {
        let root = std::path::absolute(data.root()).unwrap_or_else(|_| data.root().to_path_buf());
        let default = dirs::data_dir().map(|d| d.join("Sakshi"));
        let args = if default.as_deref() == Some(root.as_path()) {
            Vec::new()
        } else {
            vec!["--data-dir".into(), root.to_string_lossy().into_owned()]
        };
        Self { command, args }
    }

    fn command_str(&self) -> String {
        self.command.to_string_lossy().into_owned()
    }

    fn matches(&self, command: Option<&str>, args: &[String]) -> bool {
        command == Some(self.command_str().as_str()) && args == self.args
    }
}

/// Finds the `sakshi-mcp` binary: `$SAKSHI_MCP_BIN`, else next to the
/// running executable (the bundled app, or `target/<profile>/` in a
/// checkout), else one folder up (test binaries in `target/<profile>/deps/`).
pub fn locate_mcp_server() -> Result<PathBuf, AppError> {
    if let Some(path) = std::env::var_os(MCP_BIN_ENV).filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let name = format!("sakshi-mcp{}", std::env::consts::EXE_SUFFIX);
    let exe = std::env::current_exe().map_err(|e| AppError::io("the running executable", e))?;
    exe.parent()
        .into_iter()
        .flat_map(|dir| dir.ancestors().take(2))
        .map(|dir| dir.join(&name))
        .find(|path| path.is_file())
        .map(|path| fs::canonicalize(&path).unwrap_or(path))
        .ok_or_else(|| {
            AppError::Invalid(format!(
                "can't find {name} next to {}; build it with \
                 `cargo build -p sakshi-mcp-server` or set {MCP_BIN_ENV}",
                exe.display()
            ))
        })
}

/// Where the clients keep their configuration. [`ClientPaths::detect`]
/// finds the real files; tests point them at a temporary folder.
#[derive(Debug, Clone)]
pub struct ClientPaths {
    pub claude_desktop: PathBuf,
    pub claude_code: PathBuf,
    pub codex: PathBuf,
    pub cursor: PathBuf,
}

impl ClientPaths {
    pub fn detect() -> Result<Self, AppError> {
        let home = dirs::home_dir()
            .ok_or_else(|| AppError::Invalid("can't find your home folder".into()))?;
        let env_dir = |name: &str| {
            std::env::var_os(name)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let config_dir = dirs::config_dir().unwrap_or_else(|| home.join(".config"));
        Ok(Self {
            claude_desktop: config_dir.join("Claude").join("claude_desktop_config.json"),
            claude_code: env_dir("CLAUDE_CONFIG_DIR")
                .unwrap_or_else(|| home.clone())
                .join(".claude.json"),
            codex: env_dir("CODEX_HOME")
                .unwrap_or_else(|| home.join(".codex"))
                .join("config.toml"),
            cursor: home.join(".cursor").join("mcp.json"),
        })
    }

    pub fn config_file(&self, client: Client) -> &Path {
        match client {
            Client::ClaudeDesktop => &self.claude_desktop,
            Client::ClaudeCode => &self.claude_code,
            Client::Codex => &self.codex,
            Client::Cursor => &self.cursor,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// The client doesn't seem to be installed.
    NotFound,
    /// Installed, without Sakshi.
    NotConfigured,
    /// Sakshi is set up with a different command or data folder.
    Different,
    Configured,
    /// The config file can't be read, e.g. it isn't valid JSON.
    Unreadable,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClientSetup {
    pub client: Client,
    pub name: &'static str,
    pub config_path: PathBuf,
    pub status: Status,
    pub can_write: bool,
    /// What to do with `snippet` to set the client up by hand.
    pub instructions: String,
    pub snippet: String,
}

/// Every client, with its status and how to set it up.
pub fn clients(launch: &ServerLaunch, paths: &ClientPaths) -> Vec<ClientSetup> {
    Client::ALL
        .into_iter()
        .map(|client| {
            let config_path = paths.config_file(client).to_path_buf();
            let instructions = match client {
                Client::ClaudeDesktop => format!(
                    "Add this to {}, then quit and reopen Claude Desktop.",
                    config_path.display()
                ),
                Client::ClaudeCode => "Run this in a terminal.".to_owned(),
                Client::Codex | Client::Cursor => {
                    format!("Add this to {}.", config_path.display())
                }
            };
            ClientSetup {
                client,
                name: client.name(),
                status: status(client, launch, paths),
                can_write: client.can_write(),
                instructions,
                snippet: snippet(client, launch),
                config_path,
            }
        })
        .collect()
}

/// What to paste into the client's config, or for Claude Code, the command
/// to run.
pub fn snippet(client: Client, launch: &ServerLaunch) -> String {
    match client {
        Client::ClaudeDesktop | Client::Cursor => {
            let doc = json!({ "mcpServers": { SERVER_NAME: server_entry(launch) } });
            serde_json::to_string_pretty(&doc).expect("a JSON value always serializes")
        }
        Client::ClaudeCode => {
            let mut words = vec![
                "claude".to_owned(),
                "mcp".into(),
                "add".into(),
                "--scope".into(),
                "user".into(),
                SERVER_NAME.into(),
                "--".into(),
                launch.command_str(),
            ];
            words.extend(launch.args.iter().cloned());
            words
                .iter()
                .map(|w| shell_quote(w))
                .collect::<Vec<_>>()
                .join(" ")
        }
        Client::Codex => {
            let mut toml = format!(
                "[mcp_servers.{SERVER_NAME}]\ncommand = {}\n",
                toml_string(&launch.command_str())
            );
            if !launch.args.is_empty() {
                toml.push_str(&format!("args = {}\n", toml_array(&launch.args)));
            }
            toml
        }
    }
}

/// Whether `client` is set up to launch `launch`.
pub fn status(client: Client, launch: &ServerLaunch, paths: &ClientPaths) -> Status {
    let path = paths.config_file(client);
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let installed = match client {
                // Claude Code may not have written ~/.claude.json yet.
                Client::ClaudeCode => path.with_file_name(".claude").is_dir(),
                _ => path.parent().is_some_and(Path::is_dir),
            };
            return if installed {
                Status::NotConfigured
            } else {
                Status::NotFound
            };
        }
        Err(_) => return Status::Unreadable,
    };
    match client {
        Client::Codex => codex_status(&text, launch),
        _ => match parse_config(&text) {
            Ok(doc) => json_status(&doc, launch),
            Err(_) => Status::Unreadable,
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum InstallOutcome {
    /// Nothing to change.
    AlreadyConfigured,
    /// The config file was updated; `backup` is a copy of what was there.
    Written {
        path: PathBuf,
        backup: Option<PathBuf>,
    },
}

/// Adds Sakshi to the config file of a client that [`Client::can_write`],
/// or points an existing `sakshi` entry at `launch`. Other servers and
/// settings in the file are left as they are, and the old file is kept as
/// `<name>.sakshi-backup-<time>`.
pub fn install(
    client: Client,
    launch: &ServerLaunch,
    paths: &ClientPaths,
) -> Result<InstallOutcome, AppError> {
    if !client.can_write() {
        return Err(AppError::Invalid(format!(
            "Sakshi doesn't edit {}'s configuration; {}",
            client.name(),
            match client {
                Client::ClaudeCode => "run the command it shows instead",
                _ => "paste the snippet it shows instead",
            }
        )));
    }
    // Write through a symlinked config (e.g. one kept in a dotfiles repo).
    let path = paths.config_file(client);
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let existing = match fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(AppError::io(&path, e)),
    };
    let mut doc = match &existing {
        Some(text) => parse_config(text).map_err(|why| {
            AppError::Invalid(format!(
                "{} {why}; fix it or add Sakshi by hand",
                path.display()
            ))
        })?,
        None => Map::new(),
    };
    if json_status(&doc, launch) == Status::Configured {
        return Ok(InstallOutcome::AlreadyConfigured);
    }

    let servers = doc
        .entry("mcpServers")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| {
            AppError::Invalid(format!(
                "\"mcpServers\" in {} isn't an object; fix it or add Sakshi by hand",
                path.display()
            ))
        })?;
    match servers.get_mut(SERVER_NAME).and_then(Value::as_object_mut) {
        // Keep anything else the user set on the entry, such as "env".
        Some(entry) => {
            entry.insert("command".into(), launch.command_str().into());
            if launch.args.is_empty() {
                entry.shift_remove("args");
            } else {
                entry.insert("args".into(), launch.args.clone().into());
            }
        }
        None => {
            servers.insert(SERVER_NAME.into(), server_entry(launch));
        }
    }

    let backup = match &existing {
        Some(_) => Some(back_up(&path)?),
        None => None,
    };
    let permissions = fs::metadata(&path).ok().map(|m| m.permissions());
    write_json_atomic(&path, &doc)?;
    if let Some(permissions) = permissions {
        fs::set_permissions(&path, permissions).map_err(|e| AppError::io(&path, e))?;
    }
    Ok(InstallOutcome::Written { path, backup })
}

fn server_entry(launch: &ServerLaunch) -> Value {
    let mut entry = Map::new();
    entry.insert("command".into(), launch.command_str().into());
    if !launch.args.is_empty() {
        entry.insert("args".into(), launch.args.clone().into());
    }
    Value::Object(entry)
}

/// A client's JSON config. An empty file counts as an empty object.
fn parse_config(text: &str) -> Result<Map<String, Value>, String> {
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str(text) {
        Ok(Value::Object(doc)) => Ok(doc),
        Ok(_) => Err("isn't a JSON object".into()),
        Err(e) => Err(format!("isn't valid JSON ({e})")),
    }
}

fn json_status(doc: &Map<String, Value>, launch: &ServerLaunch) -> Status {
    let Some(entry) = doc
        .get("mcpServers")
        .and_then(|servers| servers.get(SERVER_NAME))
    else {
        return Status::NotConfigured;
    };
    let command = entry.get("command").and_then(Value::as_str);
    let args: Vec<String> = entry
        .get("args")
        .and_then(Value::as_array)
        .map(|args| {
            args.iter()
                .map(|a| a.as_str().unwrap_or_default().to_owned())
                .collect()
        })
        .unwrap_or_default();
    if launch.matches(command, &args) {
        Status::Configured
    } else {
        Status::Different
    }
}

/// Codex's TOML is only read, never written, so a line-based look at the
/// `[mcp_servers.sakshi]` table is enough: the command and args must be
/// written the way [`snippet`] writes them to count as configured.
fn codex_status(text: &str, launch: &ServerLaunch) -> Status {
    let header = format!("[mcp_servers.{SERVER_NAME}]");
    let mut lines = text.lines().map(str::trim);
    if !lines.any(|line| line == header) {
        return Status::NotConfigured;
    }
    let mut command = None;
    let mut args = None;
    for line in lines.take_while(|line| !line.starts_with('[')) {
        if let Some((key, value)) = line.split_once('=') {
            match key.trim() {
                "command" => command = Some(value.trim().to_owned()),
                "args" => args = Some(value.trim().to_owned()),
                _ => {}
            }
        }
    }
    let want_args = (!launch.args.is_empty()).then(|| toml_array(&launch.args));
    let args = args.filter(|a| a != "[]");
    if command == Some(toml_string(&launch.command_str())) && args == want_args {
        Status::Configured
    } else {
        Status::Different
    }
}

/// A TOML basic string. JSON's string escapes are all valid TOML.
fn toml_string(s: &str) -> String {
    serde_json::to_string(s).expect("a string always serializes")
}

fn toml_array(items: &[String]) -> String {
    let items: Vec<String> = items.iter().map(|i| toml_string(i)).collect();
    format!("[{}]", items.join(", "))
}

fn shell_quote(word: &str) -> String {
    let safe = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:=@+,%".contains(c));
    if safe {
        word.to_owned()
    } else if cfg!(windows) {
        format!("\"{}\"", word.replace('"', "\\\""))
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

fn back_up(path: &Path) -> Result<PathBuf, AppError> {
    let stamp = jiff::Zoned::now().strftime("%Y%m%d-%H%M%S");
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".sakshi-backup-{stamp}"));
    let backup = path.with_file_name(name);
    fs::copy(path, &backup).map_err(|e| AppError::io(&backup, e))?;
    Ok(backup)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(root: &Path) -> ClientPaths {
        ClientPaths {
            claude_desktop: root.join("Claude").join("claude_desktop_config.json"),
            claude_code: root.join(".claude.json"),
            codex: root.join(".codex").join("config.toml"),
            cursor: root.join(".cursor").join("mcp.json"),
        }
    }

    fn launch(args: &[&str]) -> ServerLaunch {
        ServerLaunch {
            command: PathBuf::from("/Applications/Sakshi.app/Contents/MacOS/sakshi-mcp"),
            args: args.iter().map(|a| (*a).to_owned()).collect(),
        }
    }

    #[test]
    fn passes_the_data_dir_only_when_it_isnt_the_default() {
        let root = tempfile::tempdir().unwrap();
        let custom = ServerLaunch::new("/bin/sakshi-mcp".into(), &DataDir::new(root.path()));
        assert_eq!(
            custom.args,
            [
                "--data-dir".to_owned(),
                root.path().to_string_lossy().into_owned()
            ]
        );

        if let Some(default) = dirs::data_dir() {
            let launch = ServerLaunch::new(
                "/bin/sakshi-mcp".into(),
                &DataDir::new(default.join("Sakshi")),
            );
            assert!(launch.args.is_empty());
        }
    }

    #[test]
    fn parses_client_names() {
        for client in Client::ALL {
            assert_eq!(client.as_str().parse::<Client>(), Ok(client));
        }
        assert!("claude".parse::<Client>().is_err());
    }

    #[test]
    fn reports_missing_and_installed_clients() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let launch = launch(&[]);

        for client in Client::ALL {
            assert_eq!(
                status(client, &launch, &paths),
                Status::NotFound,
                "{client}"
            );
        }
        fs::create_dir_all(root.path().join("Claude")).unwrap();
        fs::create_dir_all(root.path().join(".claude")).unwrap();
        assert_eq!(
            status(Client::ClaudeDesktop, &launch, &paths),
            Status::NotConfigured
        );
        assert_eq!(
            status(Client::ClaudeCode, &launch, &paths),
            Status::NotConfigured
        );
    }

    #[test]
    fn installs_into_a_new_file() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let launch = launch(&["--data-dir", "/data"]);

        let outcome = install(Client::Cursor, &launch, &paths).unwrap();

        assert_eq!(
            outcome,
            InstallOutcome::Written {
                path: paths.cursor.clone(),
                backup: None
            }
        );
        let doc: Value = serde_json::from_str(&fs::read_to_string(&paths.cursor).unwrap()).unwrap();
        assert_eq!(
            doc,
            json!({ "mcpServers": { "sakshi": {
                "command": "/Applications/Sakshi.app/Contents/MacOS/sakshi-mcp",
                "args": ["--data-dir", "/data"],
            }}})
        );
        assert_eq!(status(Client::Cursor, &launch, &paths), Status::Configured);
        assert_eq!(
            install(Client::Cursor, &launch, &paths).unwrap(),
            InstallOutcome::AlreadyConfigured
        );
    }

    #[test]
    fn keeps_other_servers_settings_and_key_order() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let original = r#"{
  "zebra": true,
  "mcpServers": {
    "other": { "command": "other-server" },
    "sakshi": { "command": "/old/sakshi-mcp", "args": ["--data-dir", "/old"], "env": { "A": "1" } }
  },
  "alpha": 1
}
"#;
        fs::create_dir_all(paths.claude_desktop.parent().unwrap()).unwrap();
        fs::write(&paths.claude_desktop, original).unwrap();
        let launch = launch(&[]);
        assert_eq!(
            status(Client::ClaudeDesktop, &launch, &paths),
            Status::Different
        );

        let InstallOutcome::Written { backup, .. } =
            install(Client::ClaudeDesktop, &launch, &paths).unwrap()
        else {
            panic!("expected a write");
        };

        assert_eq!(fs::read_to_string(backup.unwrap()).unwrap(), original);
        let text = fs::read_to_string(&paths.claude_desktop).unwrap();
        let doc: Map<String, Value> = serde_json::from_str(&text).unwrap();
        let keys: Vec<&str> = doc.keys().map(String::as_str).collect();
        assert_eq!(keys, ["zebra", "mcpServers", "alpha"]);
        assert_eq!(
            doc["mcpServers"]["other"],
            json!({ "command": "other-server" })
        );
        assert_eq!(
            doc["mcpServers"]["sakshi"],
            json!({ "command": "/Applications/Sakshi.app/Contents/MacOS/sakshi-mcp", "env": { "A": "1" } })
        );
        assert_eq!(
            status(Client::ClaudeDesktop, &launch, &paths),
            Status::Configured
        );
    }

    #[test]
    fn refuses_to_overwrite_invalid_json() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        fs::create_dir_all(paths.cursor.parent().unwrap()).unwrap();
        fs::write(&paths.cursor, "{ \"mcpServers\": ").unwrap();

        assert_eq!(
            status(Client::Cursor, &launch(&[]), &paths),
            Status::Unreadable
        );
        let err = install(Client::Cursor, &launch(&[]), &paths).unwrap_err();
        assert!(err.to_string().contains("isn't valid JSON"), "{err}");
        assert_eq!(
            fs::read_to_string(&paths.cursor).unwrap(),
            "{ \"mcpServers\": "
        );
    }

    #[cfg(unix)]
    #[test]
    fn writes_through_a_symlink() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let real = root.path().join("dotfiles-mcp.json");
        fs::write(&real, "{}").unwrap();
        fs::create_dir_all(paths.cursor.parent().unwrap()).unwrap();
        std::os::unix::fs::symlink(&real, &paths.cursor).unwrap();

        install(Client::Cursor, &launch(&[]), &paths).unwrap();

        assert!(
            fs::symlink_metadata(&paths.cursor)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert!(fs::read_to_string(&real).unwrap().contains("sakshi-mcp"));
    }

    #[test]
    fn only_edits_json_clients() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        assert!(install(Client::ClaudeCode, &launch(&[]), &paths).is_err());
        assert!(install(Client::Codex, &launch(&[]), &paths).is_err());
        assert!(!paths.claude_code.exists());
    }

    #[test]
    fn reads_claude_code_user_servers() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let launch = launch(&[]);
        fs::write(
            &paths.claude_code,
            r#"{"numStartups": 3, "mcpServers": {"sakshi": {"type": "stdio", "command": "/Applications/Sakshi.app/Contents/MacOS/sakshi-mcp", "args": [], "env": {}}}}"#,
        )
        .unwrap();
        assert_eq!(
            status(Client::ClaudeCode, &launch, &paths),
            Status::Configured
        );
    }

    #[test]
    fn codex_snippet_round_trips_through_its_status() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let launch = ServerLaunch {
            command: PathBuf::from(r#"/Users/me/My "Apps"/sakshi-mcp"#),
            args: vec!["--data-dir".into(), "/data".into()],
        };
        let toml = snippet(Client::Codex, &launch);
        assert_eq!(
            toml,
            "[mcp_servers.sakshi]\n\
             command = \"/Users/me/My \\\"Apps\\\"/sakshi-mcp\"\n\
             args = [\"--data-dir\", \"/data\"]\n"
        );
        fs::create_dir_all(paths.codex.parent().unwrap()).unwrap();
        fs::write(
            &paths.codex,
            format!("model = \"o3\"\n\n{toml}\n[other]\nx = 1\n"),
        )
        .unwrap();
        assert_eq!(status(Client::Codex, &launch, &paths), Status::Configured);

        let moved = ServerLaunch {
            args: Vec::new(),
            ..launch
        };
        assert_eq!(status(Client::Codex, &moved, &paths), Status::Different);
    }

    #[cfg(unix)]
    #[test]
    fn claude_code_command_is_shell_quoted() {
        let launch = ServerLaunch {
            command: PathBuf::from("/Users/me/My Apps/sakshi-mcp"),
            args: vec!["--data-dir".into(), "/it's/here".into()],
        };
        assert_eq!(
            snippet(Client::ClaudeCode, &launch),
            r"claude mcp add --scope user sakshi -- '/Users/me/My Apps/sakshi-mcp' --data-dir '/it'\''s/here'"
        );
    }

    #[test]
    fn json_snippet_matches_what_install_writes() {
        let root = tempfile::tempdir().unwrap();
        let paths = paths(root.path());
        let launch = launch(&[]);
        install(Client::Cursor, &launch, &paths).unwrap();
        let written: Value =
            serde_json::from_str(&fs::read_to_string(&paths.cursor).unwrap()).unwrap();
        let shown: Value = serde_json::from_str(&snippet(Client::Cursor, &launch)).unwrap();
        assert_eq!(written, shown);
    }
}
