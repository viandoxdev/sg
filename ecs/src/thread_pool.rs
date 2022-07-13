use std::{
    marker::PhantomData,
    sync::{mpsc, Arc},
    thread::{self, JoinHandle},
};

use parking_lot::Mutex;
use std::sync::mpsc::{Receiver, Sender};

pub struct ThreadPool<J: Job> {
    workers: Vec<Worker<J>>,
    jobs_in: Sender<Option<J>>,
    jobs_out: Arc<Mutex<Receiver<Option<J>>>>,
}

pub struct Worker<J> {
    thread: JoinHandle<()>,
    _phantom: PhantomData<J>,
}

impl<J: Job> Worker<J> {
    fn new(jobs: Arc<Mutex<Receiver<Option<J>>>>) -> Self {
        Self {
            thread: thread::spawn(move || {
                // If recv returns an Error, then the sender has been lost and all workes should
                // exit. If the returned job is None, then thte worker should exit (None being the
                // close signal)?
                while let Ok(Some(job)) = jobs.lock().recv() {
                    job.execute();
                }
            }),
            _phantom: PhantomData,
        }
    }
}

impl<J: Job> ThreadPool<J> {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            workers: Vec::new(),
            jobs_in: sender,
            jobs_out: Arc::new(Mutex::new(receiver)),
        }
    }

    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }

    pub fn add_workers(&mut self, count: usize) {
        self.workers
            .extend(std::iter::repeat_with(|| Worker::new(self.jobs_out.clone())).take(count));
    }

    pub fn run(&self, job: J) {
        self.jobs_in
            .send(Some(job))
            .expect("Error when sending job to workers");
    }

    pub fn run_many(&self, jobs: impl IntoIterator<Item = J>) {
        for job in jobs {
            self.run(job);
        }
    }
}

impl<J: Job> Drop for ThreadPool<J> {
    fn drop(&mut self) {
        for _ in 0..self.worker_count() {
            self.jobs_in
                .send(None)
                .expect("Error when shutting down worker");
        }
        for worker in self.workers.drain(..) {
            worker.thread.join().unwrap();
        }
    }
}

pub trait Job: Send + Sync + 'static {
    fn execute(self);
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU32;

    use super::Job;

    static TOTAL: AtomicU32 = AtomicU32::new(0);

    struct J {
        data: u32,
    }

    impl Job for J {
        fn execute(self) {
            TOTAL.fetch_add(self.data, std::sync::atomic::Ordering::SeqCst);
        }
    }

    #[test]
    fn basic() {}
}
