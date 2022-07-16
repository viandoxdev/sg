use std::{
    fmt::Debug,
    marker::PhantomData,
    sync::{atomic::AtomicU32, mpsc, Arc},
    thread::{self, JoinHandle},
};

use parking_lot::{Condvar, Mutex};
use std::sync::mpsc::{Receiver, Sender};

pub struct ThreadPool<J: Job> {
    workers: Vec<Worker<J>>,
    actions: Sender<Action<J>>,
    actions_receiver: Arc<Mutex<Receiver<Action<J>>>>,
}

pub struct Worker<J> {
    thread: JoinHandle<()>,
    _phantom: PhantomData<J>,
}

enum Action<J: Job> {
    Job(J, Arc<Wait>),
    Stop,
}

impl<J: Job> Debug for Action<J> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Action::Job(..) => write!(f, "Action::Job"),
            Action::Stop => write!(f, "Action::Stop"),
        }
    }
}

impl<J: Job> Worker<J> {
    fn new(actions: Arc<Mutex<Receiver<Action<J>>>>, id: u64) -> Self {
        Self {
            thread: thread::spawn(move || {
                log::trace!("Worker({id}): Started");
                log::trace!("Worker({id}): Listening for action");
                while let Ok(action) = actions.lock().recv() {
                    log::trace!("Worker({id}): Got action {action:?}");
                    match action {
                        Action::Job(job, wait) => {
                            job.execute();
                            log::trace!("Worker({id}): Finished job");
                            // Notify once we're done
                            wait.notify();
                        }
                        Action::Stop => {
                            break;
                        }
                    }
                }
                log::trace!("Worker({id}): Stopping");
            }),
            _phantom: PhantomData,
        }
    }
}

impl<J: Job> ThreadPool<J> {
    /// Create a new thread pool with no worker
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            workers: Vec::new(),
            actions: sender,
            actions_receiver: Arc::new(Mutex::new(receiver)),
        }
    }
    /// Get the number of workers in the pool
    #[inline(always)]
    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }
    /// Add count workers to the pool
    pub fn add_workers(&mut self, count: usize) {
        let mut ids = (self.worker_count() as u64)..;
        self.workers.extend(
            std::iter::repeat_with(|| {
                Worker::new(self.actions_receiver.clone(), ids.next().unwrap())
            })
            .take(count),
        );
    }
    /// Ensures that the thread pool has at least count workers
    #[inline(always)]
    pub fn ensure_workers(&mut self, count: usize) {
        let current = self.worker_count();
        if current < count {
            self.add_workers(count - current);
        }
    }
    /// Run a job on a worker, return a Wait that will end when the job is finished
    pub fn run(&self, job: J) -> Arc<Wait> {
        let wait = Arc::new(Wait::new(1));
        self.actions
            .send(Action::Job(job, wait.clone()))
            .expect("Error when sending job to workers");
        wait
    }
    /// Run multiple jobs on in the pool, returns a Wait that will end when all jobs are finished
    pub fn run_many(&self, jobs: impl IntoIterator<Item = J>) -> Arc<Wait> {
        let iter = jobs.into_iter();
        let mut wait_size: u32 = {
            let (lower, upper) = iter.size_hint();
            upper.unwrap_or(lower).try_into().unwrap_or(0)
        };

        let wait = Arc::new(Wait::new(wait_size));
        let mut count = 0;
        for job in iter {
            count += 1;
            // If there are more jobs than expected
            if count > wait_size {
                // Update the limit
                wait_size = count + 5;
                wait.set_limit(wait_size);
            }

            self.actions
                .send(Action::Job(job, wait.clone()))
                .expect("Error when sending job to workers");
        }
        // If the hint isn't exact, we overshoot, so we correct at the end.
        if wait_size > count {
            wait.set_limit(count);
        }

        wait
    }
}

impl<J: Job> Drop for ThreadPool<J> {
    fn drop(&mut self) {
        for _ in 0..self.worker_count() {
            self.actions
                .send(Action::Stop)
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

/// A barrier like syncronizations struct, waits for a ceratin number of notifications.
pub struct Wait {
    cond: Condvar,
    count: Mutex<u32>,
    limit: AtomicU32,
}

impl Wait {
    /// Get the number of notifications before the wait ends
    pub fn limit(&self) -> u32 {
        self.limit.load(std::sync::atomic::Ordering::Relaxed)
    }
    /// Create a new Wait
    pub fn new(limit: u32) -> Self {
        Self {
            cond: Condvar::new(),
            count: Mutex::new(0),
            limit: AtomicU32::new(limit),
        }
    }
    /// Reset the counter
    pub fn reset(&self) {
        *self.count.lock() = 0;
    }
    /// Get how many notifications have been received
    pub fn count(&self) -> u32 {
        *self.count.lock()
    }
    /// Notify one
    pub fn notify(&self) {
        let mut count = self.count.lock();
        *count += 1;
        if *count == self.limit() {
            *count = 0;
            // release the lock
            drop(count);

            self.cond.notify_all();
        }
    }
    /// Change the limit of the Wait, changing the limit to a number of notifications that already
    /// has been hit will notify the waiting threads
    pub fn set_limit(&self, limit: u32) {
        self.limit.store(limit, std::sync::atomic::Ordering::SeqCst);
        if *self.count.lock() >= limit {
            self.reset();
            self.cond.notify_all();
        }
    }
    /// Wait for limit notifications
    pub fn wait(&self) {
        self.cond.wait(&mut self.count.lock());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU32;

    use parking_lot::Mutex;

    use super::*;

    // Counter used to check if a job has been run
    static TOTAL: AtomicU32 = AtomicU32::new(0);
    // Lock to force single threading (no two test run at once, as they would both increment the
    // TOTAL)
    static LOCK: Mutex<()> = Mutex::new(());

    #[derive(Clone, Copy)]
    struct J {
        data: u32,
    }

    impl Job for J {
        fn execute(self) {
            TOTAL.fetch_add(self.data, std::sync::atomic::Ordering::SeqCst);
        }
    }

    fn reset_total() {
        TOTAL.store(0, std::sync::atomic::Ordering::SeqCst);
    }

    fn assert_total(val: u32) {
        let total = TOTAL.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(val, total);
    }

    #[test]
    fn init() {
        let mut pool = ThreadPool::<J>::new();
        pool.add_workers(20);
    }

    #[test]
    fn single() {
        let _lock = LOCK.lock();
        reset_total();

        assert_total(0); // total has been reset so 0
        let mut pool = ThreadPool::new();

        let wait = pool.run(J { data: 10 });
        // Pool doesn't have any workers, so still 0
        assert_total(0);
        pool.add_workers(1);
        wait.wait();
        assert_total(10);
    }

    #[test]
    fn many() {
        let _lock = LOCK.lock();
        reset_total();
        assert_total(0); // total has been reset so 0

        let jobs = [J { data: 5 }; 10];
        let mut pool = ThreadPool::new();
        pool.add_workers(5);

        let wait = pool.run_many(jobs);
        wait.wait();
        assert_total(50);
    }
}
