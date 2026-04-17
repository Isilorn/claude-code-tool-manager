pub mod remote_paths;
pub mod session;
pub mod sftp_io;

pub use remote_paths::RemoteClaudePaths;
pub use session::{ActiveContext, SshSessionPool};
pub use sftp_io::{FileOps, LocalFileOps, SftpFileOps};

use std::sync::{Arc, Mutex};

/// Returns the appropriate FileOps implementation based on the active context.
/// - ActiveContext.machine_id = None  → LocalFileOps (std::fs)
/// - ActiveContext.machine_id = Some  → SftpFileOps (SSH session from pool)
///
/// Commands call this at the start of any operation that touches the filesystem,
/// instead of using std::fs directly.
pub fn get_file_ops(
    pool: &Arc<Mutex<SshSessionPool>>,
    active_ctx: &Arc<Mutex<ActiveContext>>,
) -> Result<Box<dyn FileOps>, String> {
    let machine_id = active_ctx
        .lock()
        .map_err(|e| e.to_string())?
        .machine_id;

    match machine_id {
        None => Ok(Box::new(LocalFileOps)),
        Some(id) => {
            let pool = pool.lock().map_err(|e| e.to_string())?;
            let session = pool
                .get(id)
                .ok_or_else(|| format!("No active SSH connection for machine {}. Please reconnect.", id))?;
            Ok(Box::new(SftpFileOps::new(session)))
        }
    }
}
