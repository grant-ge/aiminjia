use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub async fn set_im_channel_keep_awake(
    manager: State<'_, Arc<PowerAssertionManager>>,
    enabled: bool,
) -> Result<(), String> {
    manager.set_enabled(enabled)
}

pub struct PowerAssertionManager {
    #[cfg(target_os = "macos")]
    caffeinate: std::sync::Mutex<Option<std::process::Child>>,
    #[cfg(target_os = "windows")]
    worker: std::sync::Mutex<Option<WindowsPowerWorker>>,
}

impl PowerAssertionManager {
    pub fn new() -> Self {
        Self {
            #[cfg(target_os = "macos")]
            caffeinate: std::sync::Mutex::new(None),
            #[cfg(target_os = "windows")]
            worker: std::sync::Mutex::new(None),
        }
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<(), String> {
        platform_set_enabled(self, enabled)
    }
}

impl Default for PowerAssertionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "macos")]
fn platform_set_enabled(manager: &PowerAssertionManager, enabled: bool) -> Result<(), String> {
    use std::process::{Command, Stdio};

    let mut guard = manager
        .caffeinate
        .lock()
        .map_err(|_| "power assertion lock poisoned".to_string())?;

    if enabled {
        if guard.is_some() {
            return Ok(());
        }

        let child = Command::new("/usr/bin/caffeinate")
            .args(["-dimsu"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("failed to start caffeinate: {e}"))?;
        *guard = Some(child);
        return Ok(());
    }

    if let Some(mut child) = guard.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn platform_set_enabled(manager: &PowerAssertionManager, enabled: bool) -> Result<(), String> {
    let mut guard = manager
        .worker
        .lock()
        .map_err(|_| "power assertion lock poisoned".to_string())?;

    if guard.is_none() {
        *guard = Some(WindowsPowerWorker::spawn()?);
    }

    let worker = guard
        .as_ref()
        .ok_or_else(|| "power assertion worker unavailable".to_string())?;
    worker
        .set_enabled(enabled)
        .map_err(|e| format!("failed to update power assertion: {e}"))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_set_enabled(_manager: &PowerAssertionManager, enabled: bool) -> Result<(), String> {
    if enabled {
        log::warn!("IM channel keep-awake is not supported on this platform");
    }
    Ok(())
}

#[cfg(target_os = "windows")]
enum WindowsPowerCommand {
    SetEnabled(bool),
    Shutdown,
}

#[cfg(target_os = "windows")]
struct WindowsPowerWorker {
    tx: std::sync::mpsc::Sender<WindowsPowerCommand>,
    join: Option<std::thread::JoinHandle<()>>,
}

#[cfg(target_os = "windows")]
impl WindowsPowerWorker {
    fn spawn() -> Result<Self, String> {
        let (tx, rx) = std::sync::mpsc::channel::<WindowsPowerCommand>();
        let join = std::thread::Builder::new()
            .name("aijia-power-assertion".to_string())
            .spawn(move || {
                while let Ok(cmd) = rx.recv() {
                    match cmd {
                        WindowsPowerCommand::SetEnabled(enabled) => {
                            set_windows_execution_state(enabled);
                        }
                        WindowsPowerCommand::Shutdown => {
                            set_windows_execution_state(false);
                            break;
                        }
                    }
                }
                set_windows_execution_state(false);
            })
            .map_err(|e| format!("failed to spawn power assertion thread: {e}"))?;

        Ok(Self {
            tx,
            join: Some(join),
        })
    }

    fn set_enabled(
        &self,
        enabled: bool,
    ) -> Result<(), std::sync::mpsc::SendError<WindowsPowerCommand>> {
        self.tx.send(WindowsPowerCommand::SetEnabled(enabled))
    }
}

#[cfg(target_os = "windows")]
impl Drop for WindowsPowerWorker {
    fn drop(&mut self) {
        let _ = self.tx.send(WindowsPowerCommand::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

#[cfg(target_os = "windows")]
fn set_windows_execution_state(enabled: bool) {
    use windows_sys::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
    };

    let flags = if enabled {
        ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
    } else {
        ES_CONTINUOUS
    };

    unsafe {
        SetThreadExecutionState(flags);
    }
}
