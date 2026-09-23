//! Bounded, cancellable work away from the event loop. Two workers per
//! process, shared by every window; dropped views cancel their queued work.
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc::{self, Receiver, SyncSender},
    Arc, Mutex, OnceLock,
};

type Job = Box<dyn FnOnce() + Send>;

fn queue() -> &'static SyncSender<Job> {
    static QUEUE: OnceLock<SyncSender<Job>> = OnceLock::new();
    QUEUE.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<Job>(16);
        let rx = Arc::new(Mutex::new(rx));
        for i in 0..2 {
            let rx = rx.clone();
            std::thread::Builder::new()
                .name(format!("nus-work-{i}"))
                .spawn(move || loop {
                    let job = rx.lock().unwrap().recv();
                    let Ok(job) = job else { break };
                    // One bad file or third-party grammar must not kill the pool.
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
                })
                .expect("background worker");
        }
        tx
    })
}

pub struct Task<T> {
    rx: Receiver<T>,
    cancel: Arc<AtomicUsize>,
}

impl<T: Send + 'static> Task<T> {
    /// Returns None when the queue is full; the caller can retry on a later
    /// tick. Never wait for a busy worker from the UI thread.
    pub fn start(f: impl FnOnce(&AtomicUsize) -> T + Send + 'static) -> Option<Self> {
        let (tx, rx) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicUsize::new(0));
        let flag = cancel.clone();
        queue()
            .try_send(Box::new(move || {
                if flag.load(Ordering::Relaxed) != 0 {
                    return;
                }
                let value = f(&flag);
                if flag.load(Ordering::Relaxed) == 0 {
                    let _ = tx.send(value);
                    crate::browser_runtime::wake();
                }
            }))
            .ok()?;
        Some(Self { rx, cancel })
    }

    pub fn take(&self) -> Result<T, mpsc::TryRecvError> {
        self.rx.try_recv()
    }
}

impl<T> Drop for Task<T> {
    fn drop(&mut self) {
        self.cancel.store(1, Ordering::Relaxed);
    }
}
