use crate::history::HistoryRecordingEmitter;
use engine::{NodeService, SendResult};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Application state for managing sharing sessions
pub struct AppState {
    pub node: Option<Arc<NodeService>>,
    /// Set when device-node initialization fails (pairing unavailable; send/receive still work).
    pub node_init_error: Option<String>,
    pub current_share: Option<ShareHandle>,
    pub is_share_starting: bool,
    /// Cancellation invalidates the generation but retains the reservation
    /// until the in-flight import has finished and its resources are closed.
    pub(crate) share_start_generation: u64,
    pub is_transporting: bool,
    pub launch_intent: Vec<String>,
    pub current_receive_cancel: Option<tokio::sync::oneshot::Sender<()>>,
    /// Sending cancellation consumes its sender, not the receive reservation.
    pub current_receive_active: bool,
    pub current_receive_cancelled: bool,
    /// Hash of the receive in flight, so history cannot delete the partial
    /// store out from under an active download.
    pub current_receive_hash: Option<String>,
    pub last_cancelled_recv_hash: Option<String>,
    /// Completed marked text, cached immediately after validation so a later
    /// filesystem replacement cannot redirect the read command.
    pub completed_text: HashMap<String, String>,
    /// Latest unpresented result: Android may suspend the WebView before its event arrives.
    pub pending_received_text: Option<ReceivedTextReady>,
}

#[derive(Clone, serde::Serialize)]
pub struct ReceivedTextReady {
    pub ticket: String,
    pub path: String,
    pub size: usize,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            node: None,
            node_init_error: None,
            current_share: None,
            is_share_starting: false,
            share_start_generation: 0,
            is_transporting: false,
            launch_intent: Vec::new(),
            current_receive_cancel: None,
            current_receive_active: false,
            current_receive_cancelled: false,
            current_receive_hash: None,
            last_cancelled_recv_hash: None,
            completed_text: HashMap::new(),
            pending_received_text: None,
        }
    }
}

impl AppState {
    pub fn reserve_receive(&mut self, cancel: tokio::sync::oneshot::Sender<()>) -> Result<(), String> {
        if self.current_receive_active {
            return Err("Already receiving a file. Wait for the current download to finish.".to_string());
        }
        self.current_receive_active = true;
        self.current_receive_cancelled = false;
        self.current_receive_cancel = Some(cancel);
        Ok(())
    }

    pub fn request_receive_cancel(&mut self) {
        if self.current_receive_active {
            self.current_receive_cancelled = true;
        }
        if let Some(tx) = self.current_receive_cancel.take() {
            let _ = tx.send(());
        }
    }
    pub fn reserve_share_start(&mut self) -> Result<u64, String> {
        if self.current_share.is_some() || self.is_share_starting {
            return Err("Already sharing a file. Please stop current share first.".to_string());
        }
        self.share_start_generation = self.share_start_generation.wrapping_add(1);
        self.is_share_starting = true;
        Ok(self.share_start_generation)
    }

    pub fn share_start_is_current(&self, generation: u64) -> bool {
        self.is_share_starting && self.share_start_generation == generation
    }

    pub fn cancel_share_start(&mut self) {
        if self.is_share_starting {
            self.share_start_generation = self.share_start_generation.wrapping_add(1);
        }
    }

    pub fn acknowledge_received_text(&mut self, path: &str) {
        if self
            .pending_received_text
            .as_ref()
            .is_some_and(|pending| pending.path == path)
        {
            self.pending_received_text = None;
        }
    }
}

#[cfg(test)]
mod received_text_tests {
    use super::*;

    #[test]
    fn stale_acknowledgement_cannot_discard_new_text() {
        let mut state = AppState::default();
        state
            .completed_text
            .insert("new-path".into(), "message".into());
        state.pending_received_text = Some(ReceivedTextReady {
            ticket: "new-ticket".into(),
            path: "new-path".into(),
            size: 7,
        });
        state.acknowledge_received_text("old-path");
        assert!(state.pending_received_text.is_some());
        state.acknowledge_received_text("new-path");
        assert!(state.pending_received_text.is_none());
        assert_eq!(state.completed_text.get("new-path").unwrap(), "message");
    }
}

/// Handle for an active sharing session
/// CRITICAL: This struct holds the router and temp_tag which keeps the server alive
pub struct ShareHandle {
    pub ticket: String,
    pub _path: PathBuf,
    pub send_result: SendResult,
    /// Present only while history recording is enabled. Held here so
    /// `stop_sharing` can close the row a broadcast share left open.
    pub recorder: Option<Arc<HistoryRecordingEmitter>>,
    /// Owned only by typed-text shares. The blob store has its own imported
    /// copy, but retaining the source for the whole session makes lifecycle
    /// explicit and keeps cleanup deterministic.
    pub owned_source_dir: Option<OwnedSourceDir>,
}

impl ShareHandle {
    pub fn new(
        ticket: String,
        path: PathBuf,
        send_result: SendResult,
        recorder: Option<Arc<HistoryRecordingEmitter>>,
    ) -> Self {
        Self {
            ticket,
            _path: path,
            send_result,
            recorder,
            owned_source_dir: None,
        }
    }

    pub fn with_owned_source_dir(mut self, dir: OwnedSourceDir) -> Self {
        self.owned_source_dir = Some(dir);
        self
    }

    /// Stop the sharing session and free its resources.
    pub async fn stop(&mut self) -> Result<(), String> {
        use std::time::Duration;

        match tokio::time::timeout(Duration::from_secs(2), self.send_result.router.shutdown()).await
        {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                tracing::warn!("Router shutdown error: {}", e);
            }
            Err(_) => {
                tracing::warn!("Router shutdown timeout after 2 seconds");
            }
        }

        let endpoint = self.send_result.router.endpoint();
        endpoint.close().await;

        Ok(())
    }
}

pub struct OwnedSourceDir(PathBuf);

impl OwnedSourceDir {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    pub fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for OwnedSourceDir {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            if error.kind() != std::io::ErrorKind::NotFound {
                tracing::warn!(%error, "failed to remove typed-text source directory");
            }
        }
    }
}

pub type AppStateMutex = Arc<Mutex<AppState>>;

#[cfg(test)]
mod tests {
    use super::{AppState, OwnedSourceDir};

    #[test]
    fn cancelled_preparation_retains_slot_until_cleanup() {
        let mut state = AppState::default();
        let generation = state.reserve_share_start().unwrap();
        assert!(state.share_start_is_current(generation));
        state.cancel_share_start();
        assert!(!state.share_start_is_current(generation));
        assert!(state.reserve_share_start().is_err());
        // Only the owner of the completed cleanup releases the reservation.
        state.is_share_starting = false;
        let next = state.reserve_share_start().unwrap();
        assert!(state.share_start_is_current(next));
        assert!(!state.share_start_is_current(generation));
    }

    #[test]
    fn cancelling_receive_does_not_release_its_reservation() {
        let mut state = AppState::default();
        let (tx, _rx) = tokio::sync::oneshot::channel();
        state.reserve_receive(tx).unwrap();
        state.request_receive_cancel();
        let (other_tx, _other_rx) = tokio::sync::oneshot::channel();
        assert!(state.reserve_receive(other_tx).is_err());
        assert!(state.current_receive_active);
    }

    #[test]
    fn cancellation_remains_recorded_during_final_export() {
        let mut state = AppState::default();
        let (tx, rx) = tokio::sync::oneshot::channel();
        state.reserve_receive(tx).unwrap();
        drop(rx); // Payload + advisory have already finished.
        state.request_receive_cancel();
        assert!(state.current_receive_cancelled);
        assert!(state.current_receive_active);
    }

    #[test]
    fn owned_text_source_lives_until_its_guard_is_dropped() {
        let temp = tempfile::tempdir().expect("temp dir");
        let source_dir = temp.path().join("owned-text");
        std::fs::create_dir(&source_dir).expect("source dir");
        std::fs::write(source_dir.join("DashBeam Text.txt"), "literal **markdown**")
            .expect("source file");

        let guard = OwnedSourceDir::new(source_dir.clone());
        assert!(source_dir.exists());
        drop(guard);
        assert!(!source_dir.exists());
    }
}
