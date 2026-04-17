use std::path::PathBuf;

/// Mirror of ClaudePathsInternal for a remote machine.
/// All paths are built relative to the remote user's home directory.
pub struct RemoteClaudePaths {
    pub home: PathBuf,
    pub claude_dir: PathBuf,
    pub claude_json: PathBuf,
    pub global_settings: PathBuf,
    pub commands_dir: PathBuf,
    pub skills_dir: PathBuf,
    pub agents_dir: PathBuf,
}

impl RemoteClaudePaths {
    pub fn new(remote_home: &str) -> Self {
        let home = PathBuf::from(remote_home);
        let claude_dir = home.join(".claude");

        Self {
            claude_json: home.join(".claude.json"),
            global_settings: claude_dir.join("settings.json"),
            commands_dir: claude_dir.join("commands"),
            skills_dir: claude_dir.join("skills"),
            agents_dir: claude_dir.join("agents"),
            home,
            claude_dir,
        }
    }
}
