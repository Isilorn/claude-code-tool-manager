use anyhow::Result;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Abstraction over local filesystem and SFTP operations.
/// All writers (config, settings, hooks, skills, rules, memory) accept
/// &dyn FileOps so they can operate on local or remote files transparently.
pub trait FileOps: Send + Sync {
    fn read_string(&self, path: &Path) -> Result<String>;
    fn write_string(&self, path: &Path, content: &str) -> Result<()>;
    fn exists(&self, path: &Path) -> bool;
    fn create_dir_all(&self, path: &Path) -> Result<()>;
    fn copy(&self, from: &Path, to: &Path) -> Result<()>;
    fn remove_file(&self, path: &Path) -> Result<()>;
}

// ============================================================================
// Local implementation — wraps std::fs
// ============================================================================

pub struct LocalFileOps;

impl FileOps for LocalFileOps {
    fn read_string(&self, path: &Path) -> Result<String> {
        Ok(std::fs::read_to_string(path)?)
    }

    fn write_string(&self, path: &Path, content: &str) -> Result<()> {
        std::fs::write(path, content)?;
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        std::fs::create_dir_all(path)?;
        Ok(())
    }

    fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        std::fs::copy(from, to)?;
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        std::fs::remove_file(path)?;
        Ok(())
    }
}

// ============================================================================
// SFTP implementation — wraps ssh2::Session
// ============================================================================

/// Holds a reference to the SSH session. A new SFTP channel is opened for
/// each operation (SFTP channels are cheap on an existing connection).
pub struct SftpFileOps {
    session: Arc<Mutex<ssh2::Session>>,
}

impl SftpFileOps {
    pub fn new(session: Arc<Mutex<ssh2::Session>>) -> Self {
        Self { session }
    }
}

impl FileOps for SftpFileOps {
    fn read_string(&self, path: &Path) -> Result<String> {
        let session = self.session.lock()
            .map_err(|_| anyhow::anyhow!("SSH session lock poisoned"))?;
        let sftp = session.sftp()?;
        let mut file = sftp.open(path)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        Ok(content)
    }

    fn write_string(&self, path: &Path, content: &str) -> Result<()> {
        let session = self.session.lock()
            .map_err(|_| anyhow::anyhow!("SSH session lock poisoned"))?;
        let sftp = session.sftp()?;
        let mut file = sftp.create(path)?;
        file.write_all(content.as_bytes())?;
        Ok(())
    }

    fn exists(&self, path: &Path) -> bool {
        let Ok(session) = self.session.lock() else { return false };
        let Ok(sftp) = session.sftp() else { return false };
        sftp.stat(path).is_ok()
    }

    fn create_dir_all(&self, path: &Path) -> Result<()> {
        let session = self.session.lock()
            .map_err(|_| anyhow::anyhow!("SSH session lock poisoned"))?;
        let sftp = session.sftp()?;

        let mut current = PathBuf::new();
        for component in path.components() {
            current.push(component);
            if sftp.stat(&current).is_err() {
                // Ignore errors: another process may have created it concurrently
                let _ = sftp.mkdir(&current, 0o755);
            }
        }
        Ok(())
    }

    fn copy(&self, from: &Path, to: &Path) -> Result<()> {
        // Remote-to-remote copy via read + write (files are small configs)
        let content = self.read_string(from)?;
        self.write_string(to, &content)?;
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> Result<()> {
        let session = self.session.lock()
            .map_err(|_| anyhow::anyhow!("SSH session lock poisoned"))?;
        let sftp = session.sftp()?;
        sftp.unlink(path)?;
        Ok(())
    }
}
