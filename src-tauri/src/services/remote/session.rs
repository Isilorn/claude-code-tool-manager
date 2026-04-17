use anyhow::{Context, Result};
use std::collections::HashMap;
use std::io::Read;
use std::net::TcpStream;
use std::path::Path;
use std::sync::{Arc, Mutex};

/// One active SSH connection to a remote machine.
pub struct SshConnection {
    pub session: Arc<Mutex<ssh2::Session>>,
}

/// Pool of active SSH connections, keyed by machine id from the DB.
/// Stored in Tauri state as Arc<RwLock<SshSessionPool>>.
pub struct SshSessionPool {
    connections: HashMap<i64, SshConnection>,
}

impl SshSessionPool {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
        }
    }

    /// Open a new SSH connection and add it to the pool.
    /// Returns the session so the caller can inspect the host key before storing it.
    pub fn connect(
        &mut self,
        machine_id: i64,
        host: &str,
        port: u16,
        username: &str,
        auth_method: &str,
        key_path: Option<&Path>,
    ) -> Result<Arc<Mutex<ssh2::Session>>> {
        let tcp = TcpStream::connect(format!("{}:{}", host, port))
            .with_context(|| format!("Cannot reach {}:{}", host, port))?;

        let mut session = ssh2::Session::new()?;
        session.set_tcp_stream(tcp);
        session.handshake()
            .with_context(|| "SSH handshake failed")?;

        match auth_method {
            "agent" => {
                let mut agent = session.agent()?;
                agent.connect()?;
                agent.list_identities()?;
                let identities = agent.identities()?;
                let identity = identities
                    .first()
                    .ok_or_else(|| anyhow::anyhow!("No identities found in SSH agent"))?;
                agent.userauth(username, identity)
                    .with_context(|| "SSH agent authentication failed")?;
            }
            "key" => {
                let key = key_path
                    .ok_or_else(|| anyhow::anyhow!("key_path is required for key authentication"))?;
                session
                    .userauth_pubkey_file(username, None, key, None)
                    .with_context(|| format!("SSH key authentication failed for {}", username))?;
            }
            other => {
                return Err(anyhow::anyhow!("Unknown auth_method: {}", other));
            }
        }

        if !session.authenticated() {
            return Err(anyhow::anyhow!("SSH authentication failed"));
        }

        let session = Arc::new(Mutex::new(session));
        self.connections.insert(machine_id, SshConnection { session: session.clone() });

        Ok(session)
    }

    /// Retrieve an existing session from the pool.
    pub fn get(&self, machine_id: i64) -> Option<Arc<Mutex<ssh2::Session>>> {
        self.connections.get(&machine_id).map(|c| c.session.clone())
    }

    /// Close and remove a connection from the pool.
    pub fn disconnect(&mut self, machine_id: i64) {
        if let Some(conn) = self.connections.remove(&machine_id) {
            if let Ok(session) = conn.session.lock() {
                let _ = session.disconnect(None, "Closing connection", None);
            }
        }
    }

    pub fn is_connected(&self, machine_id: i64) -> bool {
        self.connections.contains_key(&machine_id)
    }

    /// Return the SHA-256 fingerprint of the remote host key as a hex string.
    /// Used for TOFU (Trust On First Use) host key verification.
    pub fn host_key_fingerprint(&self, machine_id: i64) -> Option<String> {
        let conn = self.connections.get(&machine_id)?;
        let session = conn.session.lock().ok()?;
        let hash = session.host_key_hash(ssh2::HashType::Sha256)?;
        Some(hex_encode(hash))
    }

    /// Run `echo $HOME` on the remote to discover the home directory path.
    pub fn resolve_home(&self, machine_id: i64) -> Result<String> {
        let conn = self.connections.get(&machine_id)
            .ok_or_else(|| anyhow::anyhow!("No active connection for machine {}", machine_id))?;
        let session = conn.session.lock()
            .map_err(|_| anyhow::anyhow!("SSH session lock poisoned"))?;

        let mut channel = session.channel_session()?;
        channel.exec("echo $HOME")?;

        let mut output = String::new();
        channel.read_to_string(&mut output)?;
        channel.wait_close()?;

        let home = output.trim().to_string();
        if home.is_empty() {
            Err(anyhow::anyhow!("Could not determine remote home directory"))
        } else {
            Ok(home)
        }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(":")
}
