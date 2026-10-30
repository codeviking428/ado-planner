use std::future::Future;

use tokio::runtime::Runtime;
use tokio::sync::oneshot;

/// Dedicated Tokio runtime for Azure DevOps I/O. GPUI awaits the oneshot side.
pub struct AdoRuntime {
    inner: Option<Runtime>,
}

impl AdoRuntime {
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            inner: Some(
                tokio::runtime::Builder::new_multi_thread()
                    .thread_name("ado-client")
                    .enable_all()
                    .build()?,
            ),
        })
    }

    fn runtime(&self) -> &Runtime {
        self.inner.as_ref().expect("ado runtime")
    }

    pub fn spawn_result<T, F>(&self, fut: F) -> oneshot::Receiver<T>
    where
        T: Send + 'static,
        F: Future<Output = T> + Send + 'static,
    {
        let (tx, rx) = oneshot::channel();
        self.runtime().spawn(async move {
            let value = fut.await;
            let _ = tx.send(value);
        });
        rx
    }

    pub fn handle(&self) -> tokio::runtime::Handle {
        self.runtime().handle().clone()
    }
}

impl Default for AdoRuntime {
    fn default() -> Self {
        Self::new().expect("ado tokio runtime")
    }
}

impl Drop for AdoRuntime {
    fn drop(&mut self) {
        if let Some(runtime) = self.inner.take() {
            runtime.shutdown_background();
        }
    }
}
