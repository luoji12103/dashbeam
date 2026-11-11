use engine::{AppHandle, EventEmitter};
use std::future::Future;
use std::sync::Mutex;

/// A receive is not complete until command-layer metadata validation and final
/// export have settled. Keep UI/history completion behind that same boundary.
pub struct CompletionGate {
    downstream: AppHandle,
    pending: Mutex<Option<Option<String>>>,
}

impl CompletionGate {
    pub fn new(downstream: AppHandle) -> Self {
        Self {
            downstream,
            pending: Mutex::new(None),
        }
    }

    pub fn flush(&self) {
        let pending = self.pending.lock().unwrap().take();
        if let (Some(event), Some(downstream)) = (pending, &self.downstream) {
            let result = match event {
                Some(payload) => downstream.emit_event_with_payload("receive-completed", &payload),
                None => downstream.emit_event("receive-completed"),
            };
            if let Err(error) = result {
                tracing::warn!(%error, "could not publish receive completion");
            }
        }
    }
}

impl EventEmitter for CompletionGate {
    fn emit_event(&self, name: &str) -> Result<(), String> {
        if name == "receive-completed" {
            *self.pending.lock().unwrap() = Some(None);
            return Ok(());
        }
        self.downstream
            .as_ref()
            .map_or(Ok(()), |handle| handle.emit_event(name))
    }

    fn emit_event_with_payload(&self, name: &str, payload: &str) -> Result<(), String> {
        if name == "receive-completed" {
            *self.pending.lock().unwrap() = Some(Some(payload.to_owned()));
            return Ok(());
        }
        self.downstream.as_ref().map_or(Ok(()), |handle| {
            handle.emit_event_with_payload(name, payload)
        })
    }
}

/// Start useful work immediately, alongside bounded advisory information.
/// A failed/cancelled transfer never waits for the advisory lookup. On success
/// its result is still needed to safely recognize explicitly marked text.
pub async fn alongside_advisory<T, E, M>(
    work: impl Future<Output = Result<T, E>>,
    advisory: impl Future<Output = Option<M>>,
) -> (Result<T, E>, Option<M>) {
    tokio::pin!(work);
    tokio::pin!(advisory);
    tokio::select! {
        result = &mut work => {
            let metadata = if result.is_ok() { advisory.await } else { None };
            (result, metadata)
        }
        metadata = &mut advisory => (work.await, metadata),
    }
}

/// Keep cancellation alive through both payload and advisory completion. The
/// caller retains its reservation until the work's cleanup has settled.
pub async fn cancellable_advisory<T, M, W>(
    make_work: impl FnOnce(tokio::sync::oneshot::Receiver<()>) -> W,
    advisory: impl Future<Output = Option<M>>,
    mut cancel: tokio::sync::oneshot::Receiver<()>,
) -> (anyhow::Result<T>, Option<M>)
where
    W: Future<Output = anyhow::Result<T>>,
{
    let (cancel_work, work_cancelled) = tokio::sync::oneshot::channel();
    let (cancel_lookup, lookup_cancelled) = tokio::sync::oneshot::channel();
    let lookup = async {
        tokio::select! {
            _ = lookup_cancelled => None,
            metadata = advisory => metadata,
        }
    };
    let transfer = alongside_advisory(make_work(work_cancelled), lookup);
    tokio::pin!(transfer);
    tokio::select! {
        biased;
        _ = &mut cancel => {
            let _ = cancel_work.send(());
            let _ = cancel_lookup.send(());
            let _ = transfer.await;
            (Err(anyhow::anyhow!("cancelled")), None)
        }
        result = &mut transfer => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn payload_starts_before_metadata_is_available() {
        let (started, wait_started) = tokio::sync::oneshot::channel();
        let work = async {
            started.send(()).unwrap();
            Ok::<_, ()>("downloaded")
        };
        let metadata = async {
            wait_started.await.unwrap();
            Some("text-marker")
        };
        let result =
            tokio::time::timeout(Duration::from_secs(1), alongside_advisory(work, metadata))
                .await
                .unwrap();
        assert_eq!(result, (Ok("downloaded"), Some("text-marker")));
    }

    #[tokio::test]
    async fn cancellation_does_not_wait_for_metadata() {
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            alongside_advisory(
                async { Err::<(), _>("cancelled") },
                std::future::pending::<Option<()>>(),
            ),
        )
        .await
        .unwrap();
        assert_eq!(result, (Err("cancelled"), None));
    }

    #[tokio::test]
    async fn fast_metadata_does_not_cancel_download() {
        let (ready, wait_ready) = tokio::sync::oneshot::channel();
        let work = async {
            wait_ready.await.unwrap();
            Ok::<_, ()>(42)
        };
        let metadata = async {
            ready.send(()).unwrap();
            Some(7)
        };
        assert_eq!(alongside_advisory(work, metadata).await, (Ok(42), Some(7)));
    }

    #[tokio::test]
    async fn cancel_after_payload_before_metadata_discards_text_and_settles() {
        let (cancel, cancelled) = tokio::sync::oneshot::channel();
        let (started, started_rx) = tokio::sync::oneshot::channel();
        let receive = cancellable_advisory(
            |_| async {
                started.send(()).unwrap();
                Ok(())
            },
            std::future::pending::<Option<&str>>(),
            cancelled,
        );
        let trigger_cancel = async {
            started_rx.await.unwrap();
            cancel.send(()).unwrap();
        };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(1), async {
            tokio::join!(receive, trigger_cancel)
        })
        .await
        .unwrap();
        assert_eq!(result.0.unwrap_err().to_string(), "cancelled");
        assert!(result.1.is_none());
    }

    #[test]
    fn completion_is_not_published_before_release_and_flush_is_once() {
        struct Counter(std::sync::atomic::AtomicUsize);
        impl EventEmitter for Counter {
            fn emit_event(&self, _: &str) -> Result<(), String> {
                self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Ok(())
            }
            fn emit_event_with_payload(&self, name: &str, _: &str) -> Result<(), String> {
                self.emit_event(name)
            }
        }
        let counter = std::sync::Arc::new(Counter(std::sync::atomic::AtomicUsize::new(0)));
        let gate = CompletionGate::new(Some(counter.clone()));
        gate.emit_event_with_payload("receive-completed", "{}")
            .unwrap();
        assert_eq!(counter.0.load(std::sync::atomic::Ordering::SeqCst), 0);
        gate.flush();
        gate.flush();
        assert_eq!(counter.0.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
