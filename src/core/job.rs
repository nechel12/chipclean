use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
};

use crossbeam_channel::{Receiver, Sender, TryRecvError, unbounded};

pub struct Job<T> {
    rx: Option<Receiver<T>>,
    handle: Option<JoinHandle<()>>,
    cancel: Arc<AtomicBool>,
}

impl<T: Send + 'static> Job<T> {
    pub fn spawn(
        name: &str,
        run: impl FnOnce(Arc<AtomicBool>, Sender<T>) + Send + 'static,
    ) -> Self {
        let (tx, rx) = unbounded();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let handle = thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || run(worker_cancel, tx))
            .ok();
        Self {
            rx: Some(rx),
            handle,
            cancel,
        }
    }

    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        match &self.rx {
            Some(rx) => rx.try_recv(),
            None => Err(TryRecvError::Disconnected),
        }
    }

    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn finish(&mut self) {
        self.rx = None;
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

pub fn drain<T: Send + 'static>(job: &Option<Job<T>>) -> (Vec<T>, bool) {
    let mut events = Vec::new();
    let mut disconnected = false;
    if let Some(job) = job {
        loop {
            match job.try_recv() {
                Ok(event) => events.push(event),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    disconnected = true;
                    break;
                }
            }
        }
    }
    (events, disconnected)
}
