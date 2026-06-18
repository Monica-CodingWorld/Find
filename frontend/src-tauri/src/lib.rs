//! Tauri sidecar supervision for Find desktop application.
//! Spawns, health-checks, and supervises the FastAPI backend process.

use tauri::Manager;
use tauri::Emitter;
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Shared state tracking the backend sidecar process handle and running status.
#[derive(Default)]
struct BackendState {
    running: bool,
    shutting_down: bool,
    child: Option<CommandChild>,
}

type SharedState = Arc<Mutex<BackendState>>;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state: SharedState = Arc::new(Mutex::new(BackendState::default()));
    let state_for_exit = state.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(move |app| {
            app.manage(state.clone());
            let app_handle = app.handle().clone();
            let state_clone = state.clone();
            tauri::async_runtime::spawn(async move {
                supervise_backend(app_handle, state_clone).await;
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(move |_app_handle, event| {
            if let tauri::RunEvent::Exit = event {
                log::info!("App exiting - terminating backend sidecar.");
                let mut s = state_for_exit.lock().unwrap();
                s.shutting_down = true;
                if let Some(child) = s.child.take() {
                    if let Err(e) = child.kill() {
                        log::error!("Failed to kill backend sidecar: {}", e);
                    } else {
                        log::info!("Backend sidecar killed.");
                    }
                }
                s.running = false;
            }
        });
}

/// Supervises the backend sidecar process with automatic restart on crash.
///
/// Retries up to `MAX_RETRIES` times with a `RETRY_DELAY_SECS` delay between attempts.
/// Emits `backend-failed` event to the frontend if all retries are exhausted.
async fn supervise_backend(app: tauri::AppHandle, state: SharedState) {
    const MAX_RETRIES: u32 = 5;
    const RETRY_DELAY_SECS: u64 = 2;
    let mut retry_count = 0;
    loop {
        log::info!("Starting backend sidecar (attempt {})...", retry_count + 1);
        match start_sidecar(&app, &state).await {
            Ok(_) => {
                log::info!("Backend sidecar exited cleanly.");
                state.lock().unwrap().running = false;
                break;
            }
            Err(e) => {
                state.lock().unwrap().running = false;
                if state.lock().unwrap().shutting_down {
                    log::info!("App is shutting down - stopping supervisor.");
                    break;
                }
                retry_count += 1;
                log::error!("Backend crashed: {}. Retry {}/{}", e, retry_count, MAX_RETRIES);
                if retry_count >= MAX_RETRIES {
                    log::error!("Backend failed {} times - giving up.", MAX_RETRIES);
                    let _ = app.emit("backend-failed", "Backend crashed too many times");
                    break;
                }
                tokio::time::sleep(Duration::from_secs(RETRY_DELAY_SECS)).await;
            }
        }
    }
}

/// Polls the backend health endpoint until it responds or times out.
///
/// Returns `Ok(())` if the backend is healthy, `Err` if it does not respond within the timeout.
async fn wait_for_health() -> Result<(), String> {
    const HEALTH_URL: &str = "http://127.0.0.1:8000/health";
    const MAX_ATTEMPTS: u32 = 30;
    const POLL_INTERVAL_MS: u64 = 500;

    for attempt in 1..=MAX_ATTEMPTS {
        tokio::time::sleep(Duration::from_millis(POLL_INTERVAL_MS)).await;
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {}", e))?;
        match client.get(HEALTH_URL).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(body) = resp.text().await {
                    if body.contains("healthy") {
                        log::info!("Backend health check passed on attempt {}.", attempt);
                        return Ok(());
                    }
                }
            }
            _ => {
                log::info!("Health check attempt {}/{} - not ready yet.", attempt, MAX_ATTEMPTS);
            }
        }
    }
    Err(format!("Backend did not become healthy after {} attempts.", MAX_ATTEMPTS))
}

/// Spawns the backend sidecar, waits for health, then monitors output until termination.
///
/// Emits `backend-ready` to the frontend only after the health check passes.
/// Returns `Ok(())` on clean exit (code 0), or `Err` on crash or unexpected channel closure.
async fn start_sidecar(app: &tauri::AppHandle, state: &SharedState) -> Result<(), String> {
    let shell = app.shell();
    let (mut rx, child) = shell
        .sidecar("find-backend")
        .map_err(|e| format!("Failed to create sidecar: {}", e))?
        .spawn()
        .map_err(|e| format!("Failed to spawn sidecar: {}", e))?;

    {
        let mut s = state.lock().unwrap();
        s.running = true;
        s.child = Some(child);
    }

    log::info!("Backend sidecar spawned. Waiting for health check...");

    match wait_for_health().await {
        Ok(_) => {
            log::info!("Backend is healthy - emitting backend-ready.");
            let _ = app.emit("backend-ready", ());
        }
        Err(e) => {
            log::error!("Health check failed: {}", e);
            let mut s = state.lock().unwrap();
            if let Some(child) = s.child.take() {
                let _ = child.kill();
                log::info!("Killed unhealthy backend sidecar.");
            }
            s.running = false;
            return Err(e);
        }
    }

    while let Some(event) = rx.recv().await {
        match event {
            CommandEvent::Stdout(line) => log::info!("[backend] {}", String::from_utf8_lossy(&line).trim()),
            CommandEvent::Stderr(line) => log::warn!("[backend err] {}", String::from_utf8_lossy(&line).trim()),
            CommandEvent::Error(err) => return Err(format!("Sidecar error: {}", err)),
            CommandEvent::Terminated(payload) => {
                let code = payload.code.unwrap_or(-1);
                return if code == 0 { Ok(()) } else { Err(format!("Exited with code {}", code)) };
            }
            _ => {}
        }
    }

    Err("Sidecar channel closed unexpectedly.".to_string())
}
