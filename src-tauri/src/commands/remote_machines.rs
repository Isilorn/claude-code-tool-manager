use crate::db::models::{CreateRemoteMachineRequest, RemoteMachine};
use crate::db::Database;
use crate::services::remote::{ActiveContext, SshSessionPool};
use std::sync::{Arc, Mutex};
use tauri::State;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestConnectionResult {
    pub host_key_fingerprint: String,
    pub needs_confirmation: bool,
    pub remote_home: Option<String>,
}

// ============================================================================
// CRUD — remote machines
// ============================================================================

#[tauri::command]
pub fn list_remote_machines(
    db: State<'_, Arc<Mutex<Database>>>,
) -> Result<Vec<RemoteMachine>, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    db.get_all_remote_machines().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_remote_machine(
    db: State<'_, Arc<Mutex<Database>>>,
    request: CreateRemoteMachineRequest,
) -> Result<RemoteMachine, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    db.create_remote_machine(&request).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_remote_machine(
    db: State<'_, Arc<Mutex<Database>>>,
    id: i64,
    request: CreateRemoteMachineRequest,
) -> Result<RemoteMachine, String> {
    let db = db.lock().map_err(|e| e.to_string())?;
    db.update_remote_machine(id, &request).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_remote_machine(
    db: State<'_, Arc<Mutex<Database>>>,
    pool: State<'_, Arc<Mutex<SshSessionPool>>>,
    active_ctx: State<'_, Arc<Mutex<ActiveContext>>>,
    id: i64,
) -> Result<(), String> {
    // Drop session if open
    pool.lock().map_err(|e| e.to_string())?.disconnect(id);

    // Switch back to local if this was the active machine
    let mut ctx = active_ctx.lock().map_err(|e| e.to_string())?;
    if ctx.machine_id == Some(id) {
        ctx.machine_id = None;
    }
    drop(ctx);

    let db = db.lock().map_err(|e| e.to_string())?;
    db.delete_remote_machine(id).map_err(|e| e.to_string())
}

// ============================================================================
// Connection — test + TOFU host key verification
// ============================================================================

/// Connect to a remote machine and verify its host key.
///
/// First call (confirmed_key = null):
///   - Connects and returns the fingerprint with needs_confirmation = true
///     if the host key is unknown.
///   - If the stored key matches, proceeds immediately (needs_confirmation = false).
///
/// Second call (confirmed_key = "<fingerprint>"):
///   - User confirmed the key in the UI. Stores it, resolves $HOME, updates DB.
#[tauri::command]
pub fn test_remote_connection(
    db: State<'_, Arc<Mutex<Database>>>,
    pool: State<'_, Arc<Mutex<SshSessionPool>>>,
    id: i64,
    confirmed_key: Option<String>,
) -> Result<TestConnectionResult, String> {
    // Load machine config from DB
    let machine = {
        let db = db.lock().map_err(|e| e.to_string())?;
        db.get_remote_machine(id).map_err(|e| e.to_string())?
    };

    // Connect (pool lock released immediately after)
    {
        let key_path_str = machine.key_path.clone();
        let key_path = key_path_str.as_deref().map(std::path::Path::new);
        pool.lock()
            .map_err(|e| e.to_string())?
            .connect(
                id,
                &machine.host,
                machine.port as u16,
                &machine.username,
                &machine.auth_method,
                key_path,
            )
            .map_err(|e| e.to_string())?;
    }

    // Read fingerprint
    let fingerprint = pool
        .lock()
        .map_err(|e| e.to_string())?
        .host_key_fingerprint(id)
        .ok_or_else(|| "Could not read remote host key".to_string())?;

    // TOFU check
    match &machine.known_host_key {
        None => {
            // Host not yet known
            match confirmed_key {
                None => {
                    // First attempt — ask frontend to confirm
                    pool.lock().map_err(|e| e.to_string())?.disconnect(id);
                    return Ok(TestConnectionResult {
                        host_key_fingerprint: fingerprint,
                        needs_confirmation: true,
                        remote_home: None,
                    });
                }
                Some(ref confirmed) => {
                    if confirmed != &fingerprint {
                        pool.lock().map_err(|e| e.to_string())?.disconnect(id);
                        return Err("Host key mismatch during confirmation".to_string());
                    }
                    // Confirmed — proceed
                }
            }
        }
        Some(stored) => {
            // Known host — verify key hasn't changed
            if stored != &fingerprint {
                pool.lock().map_err(|e| e.to_string())?.disconnect(id);
                return Err(format!(
                    "Host key has changed (stored: {}, got: {}). \
                     Refusing connection — possible man-in-the-middle attack.",
                    stored, fingerprint
                ));
            }
        }
    }

    // Resolve remote $HOME
    let remote_home = pool
        .lock()
        .map_err(|e| e.to_string())?
        .resolve_home(id)
        .map_err(|e| e.to_string())?;

    // Persist key + home + last_connected_at to DB
    {
        let db = db.lock().map_err(|e| e.to_string())?;
        db.update_remote_machine_connection(id, Some(&fingerprint), Some(&remote_home))
            .map_err(|e| e.to_string())?;
    }

    Ok(TestConnectionResult {
        host_key_fingerprint: fingerprint,
        needs_confirmation: false,
        remote_home: Some(remote_home),
    })
}

#[tauri::command]
pub fn disconnect_remote_machine(
    pool: State<'_, Arc<Mutex<SshSessionPool>>>,
    active_ctx: State<'_, Arc<Mutex<ActiveContext>>>,
    id: i64,
) -> Result<(), String> {
    pool.lock().map_err(|e| e.to_string())?.disconnect(id);

    let mut ctx = active_ctx.lock().map_err(|e| e.to_string())?;
    if ctx.machine_id == Some(id) {
        ctx.machine_id = None;
    }
    Ok(())
}

// ============================================================================
// Active context — which machine is currently selected
// ============================================================================

#[tauri::command]
pub fn set_active_machine(
    active_ctx: State<'_, Arc<Mutex<ActiveContext>>>,
    machine_id: Option<i64>,
) -> Result<(), String> {
    active_ctx
        .lock()
        .map_err(|e| e.to_string())?
        .machine_id = machine_id;
    Ok(())
}

#[tauri::command]
pub fn get_active_machine(
    active_ctx: State<'_, Arc<Mutex<ActiveContext>>>,
) -> Result<Option<i64>, String> {
    Ok(active_ctx.lock().map_err(|e| e.to_string())?.machine_id)
}
