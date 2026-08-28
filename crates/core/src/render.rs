use std::fmt;
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};

use onionskin_render::PageRenderGeometry;

#[derive(Debug)]
pub enum WorkerError {
    Spawn(std::io::Error),
    Render(onionskin_render::RenderError),
    Stopped,
}

impl fmt::Display for WorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(error) => write!(f, "starting render worker: {error}"),
            Self::Render(error) => write!(f, "render worker: {error}"),
            Self::Stopped => write!(f, "render worker stopped before answering"),
        }
    }
}

impl std::error::Error for WorkerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn(error) => Some(error),
            Self::Render(error) => Some(error),
            Self::Stopped => None,
        }
    }
}

enum Request {
    Geometry {
        page: usize,
        response: mpsc::SyncSender<Result<PageRenderGeometry, WorkerError>>,
    },
    Shutdown,
}

pub(crate) struct WorkerHandle {
    requests: mpsc::Sender<Request>,
    thread: Option<JoinHandle<()>>,
}

impl WorkerHandle {
    pub(crate) fn spawn(bytes: Arc<Vec<u8>>) -> Result<Self, WorkerError> {
        let (requests, incoming) = mpsc::channel();
        let (ready, initialized) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("onionskin-render".into())
            .spawn(move || {
                let document = match onionskin_render::Document::from_shared(bytes) {
                    Ok(document) => {
                        let _ = ready.send(Ok(()));
                        document
                    }
                    Err(error) => {
                        let _ = ready.send(Err(WorkerError::Render(error)));
                        return;
                    }
                };

                while let Ok(request) = incoming.recv() {
                    match request {
                        Request::Geometry { page, response } => {
                            let result = document.page_geometry(page).map_err(WorkerError::Render);
                            let _ = response.send(result);
                        }
                        Request::Shutdown => break,
                    }
                }
            })
            .map_err(WorkerError::Spawn)?;

        match initialized.recv().map_err(|_| WorkerError::Stopped)? {
            Ok(()) => Ok(Self {
                requests,
                thread: Some(thread),
            }),
            Err(error) => {
                let _ = thread.join();
                Err(error)
            }
        }
    }

    pub(crate) fn page_geometry(&self, page: usize) -> Result<PageRenderGeometry, WorkerError> {
        let (response, result) = mpsc::sync_channel(1);
        self.requests
            .send(Request::Geometry { page, response })
            .map_err(|_| WorkerError::Stopped)?;
        result.recv().map_err(|_| WorkerError::Stopped)?
    }
}

impl Drop for WorkerHandle {
    fn drop(&mut self) {
        let _ = self.requests.send(Request::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
