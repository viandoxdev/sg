use std::{
    any::{Any, TypeId},
    collections::{HashMap, HashSet},
    sync::{atomic::AtomicU64, Arc},
};

use slotmap::{SecondaryMap, SlotMap};

use crate::{
    system::{IntoSystem, RequirementsMappings, System},
    thread_pool::{Job, ThreadPool, Wait},
    World,
};

#[derive(Clone, Copy)]
pub struct ExecutionContext<'a> {
    pub executor: &'a Executor,
    pub world: &'a World,
}

// Impl send and sync as the ExecutionContext will only be used when scheduled systems have been
// proven to be safely parallelizable.
unsafe impl<'a> Send for ExecutionContext<'a> {}
unsafe impl<'a> Sync for ExecutionContext<'a> {}

struct ExecutorJob {
    steps: Vec<Step>,
    waits: Arc<Vec<Wait>>,
    // TODO: remove 'static
    context: Arc<ExecutionContext<'static>>,
}

impl Job for ExecutorJob {
    fn execute(self) {
        for step in self.steps {
            match step {
                Step::Wait(index) => {
                    log::trace!("ExecutorWorker: Waiting ({index})");
                    self.waits[index].wait();
                }
                Step::Notify(index) => {
                    log::trace!("ExecutorWorker: notifying ({index})");
                    self.waits[index].notify();
                }
                Step::Run(id) => {
                    log::trace!("ExecutorWorker: running ({id:?})");
                    let system = self.context.executor.get_system(id).unwrap();
                    // SAFETY: Run Steps only exist in schedules, and schedules enforce no
                    // aliasing.
                    unsafe {
                        system.run(&self.context);
                    }
                }
            }
        }
    }
}

pub trait Resource: 'static + Any + Send {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
impl<T: 'static + Any + Send> Resource for T {
    #[inline(always)]
    fn as_any(&self) -> &dyn Any {
        self
    }
    #[inline(always)]
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

slotmap::new_key_type! {
    pub struct SystemId;
}

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct ExecutorId {
    id: u64,
}

static EXECUTOR_IDS: AtomicU64 = AtomicU64::new(0);
impl ExecutorId {
    fn new() -> Self {
        Self {
            id: EXECUTOR_IDS.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
        }
    }
}

impl Default for ExecutorId {
    fn default() -> Self {
        Self::new()
    }
}

/// A struct holding systems and resources
pub struct Executor {
    id: ExecutorId,
    resources: HashMap<TypeId, Box<dyn Resource>>,
    systems: SlotMap<SystemId, System>,
    mappings: RequirementsMappings,
    thread_pool: ThreadPool<ExecutorJob>,
}

impl Executor {
    pub fn new() -> Self {
        Self {
            id: ExecutorId::new(),
            resources: HashMap::new(),
            systems: SlotMap::with_key(),
            mappings: RequirementsMappings::new(),
            thread_pool: ThreadPool::new(),
        }
    }

    pub fn add_resource<T: Resource>(&mut self, res: T) {
        if self.resources.contains_key(&TypeId::of::<T>()) {
            panic!(
                "Trying to add resource that is already in executor: {}",
                std::any::type_name::<T>()
            );
        }
        self.resources.insert(res.type_id(), Box::new(res));
    }

    pub fn get_resource<T: Resource>(&self) -> Option<&T> {
        self.resources
            .get(&TypeId::of::<T>())
            // For some reason the as_ref here is absolutely necessary
            .and_then(|boxed| boxed.as_ref().as_any().downcast_ref::<T>())
    }
    /// Get a mutable reference to a resource without any checks for aliasing.
    ///
    /// # Safety
    ///
    /// This bypasses rust aliasing checks, and is UB if the resource is already borrowed somewhere
    /// else.
    pub unsafe fn get_resource_mut_unchecked<T: Resource>(&self) -> Option<&mut T> {
        #[allow(clippy::cast_ref_to_mut)]
        let s = &mut *(self as *const Self as *mut Self);
        s.resources
            .get_mut(&TypeId::of::<T>())
            // For some reason the as_mut here is absolutely necessary
            .and_then(|boxed| boxed.as_mut().as_any_mut().downcast_mut::<T>())
    }
    pub fn get_resource_mut<T: Resource>(&mut self) -> Option<&mut T> {
        self.resources
            .get_mut(&TypeId::of::<T>())
            .and_then(|boxed| boxed.as_mut().as_any_mut().downcast_mut::<T>())
    }
    /// Get a scheduler used to build a schedule
    pub fn schedule(&mut self) -> Scheduler {
        Scheduler {
            executor: self,
            systems: Vec::new(),
        }
    }
    /// Create a schedule for a single system
    pub fn schedule_single<A>(&mut self, sys: impl IntoSystem<A>) -> Schedule {
        self.schedule().then(sys).build()
    }
    /// Add a system to an executor. This should only be used when storing systems isn't possible
    /// and one needs a genericles handle to a system.
    ///
    /// # Note
    ///
    /// Calling this multiple times with the same system returns a new id every time.
    pub fn add_system<A>(&mut self, sys: impl IntoSystem<A>) -> SystemId {
        self.systems.insert(sys.into_system(&mut self.mappings))
    }
    fn get_system(&self, sys: SystemId) -> Option<&System> {
        self.systems.get(sys)
    }
    /// Run a given schedule against this executor and a world
    ///
    /// # Panics
    ///
    /// Panics if the schedule wasn't built from this executor
    pub fn execute(&mut self, schedule: &Schedule, world: &mut World) {
        if schedule.executor_id != self.id {
            panic!("Schedule wasn't built from correct executor");
        }
        // Make sure we have enough workers
        self.thread_pool.ensure_workers(schedule.threads.len());

        let context = Arc::new(ExecutionContext {
            executor: self,
            world,
        });
        let jobs = schedule.threads.iter().map(|thread| {
            ExecutorJob {
                waits: schedule.waits.clone(),
                // Transmute lifetime into static
                // TODO: remove that once I've found a better way
                context: unsafe { std::mem::transmute(context.clone()) },
                steps: thread.to_vec(),
            }
        });

        self.thread_pool.run_many(jobs).wait();
    }
    /// Execute a single system, note that the prefered mean of execution should be a schedule.
    pub fn execute_single<A>(&mut self, sys: impl IntoSystem<A>, world: &mut World) {
        let sys = sys.into_system(&mut self.mappings);
        let context = ExecutionContext {
            executor: self,
            world,
        };
        // SAFETY: mutable borrow of both the world and the executor guarentee no aliasing for the
        // system.
        unsafe {
            sys.run(&context);
        }
    }
}

pub struct Scheduler<'a> {
    executor: &'a mut Executor,
    systems: Vec<SystemId>,
}

impl<'a> Scheduler<'a> {
    /// Add a system to the building schedule
    pub fn then<A>(mut self, sys: impl IntoSystem<A>) -> Self {
        self.systems.push(self.executor.add_system(sys));
        self
    }
    /// Add a registred system to the building schedule. This sould be avoided in favor of
    /// Scheduler::then.
    ///
    /// # Panics
    ///
    /// This panics if the system is already in the schedule, or if the system isn't registered in
    /// the executor.
    pub fn then_by_id(mut self, sys: SystemId) -> Self {
        if self.executor.get_system(sys).is_none() {
            panic!("System isn't registered in executor");
        }
        if self.systems.contains(&sys) {
            panic!("System is already in schedule");
        }
        self.systems.push(sys);
        self
    }
    /// Run the closure F with the scheduler
    #[inline(always)]
    pub fn with<F: FnOnce(Self) -> Self>(self, f: F) -> Self {
        f(self)
    }
    /// Create a schedule from the added systems, the schedule is parallelized as much as possible
    /// while keeping the same behaviour as if the systems were run sequentially.
    ///
    /// # Note
    ///
    /// Fairely expensive, and unoptimized, should only be called a few times
    pub fn build(mut self) -> Schedule {
        if self.systems.is_empty() {
            return Schedule {
                executor_id: self.executor.id,
                threads: Arc::new(Vec::new()),
                waits: Arc::new(Vec::new()),
            };
        }

        let mut deps: SecondaryMap<SystemId, Vec<SystemId>> = SecondaryMap::new();
        let mut depths: SecondaryMap<SystemId, u32> = SecondaryMap::new();

        // find dependencies between systems
        for (i, sys_id) in self.systems.iter().enumerate() {
            let sys = self.executor.get_system(*sys_id).unwrap();
            deps.insert(*sys_id, Vec::new());
            // Loop over all the systems that come before
            for other_id in &self.systems[0..i] {
                let other = self.executor.get_system(*other_id).unwrap();

                if sys.depends_on(other) {
                    deps.get_mut(*sys_id).unwrap().push(*other_id)
                }
            }
        }
        // remove implicit dependencies
        for sys_id in &self.systems {
            // Take the dependencies from the map (and convert to set)
            let mut sys_deps = deps
                .remove(*sys_id)
                .unwrap()
                .into_iter()
                .collect::<HashSet<_>>();

            for dep in sys_deps.clone() {
                // A set of all the systems this dependencies implies
                let mut implies: HashSet<SystemId> = HashSet::new();

                // Get all the depenencies of this dep, including sub depenencies
                fn recurse_dependencies(
                    id: SystemId,
                    set: &mut HashSet<SystemId>,
                    deps: &SecondaryMap<SystemId, Vec<SystemId>>,
                ) {
                    for dep in &deps[id] {
                        set.insert(id);
                        recurse_dependencies(*dep, set, deps);
                    }
                }
                recurse_dependencies(dep, &mut implies, &deps);
                // remove all implied depenencies from the original dep list
                for implied in implies {
                    sys_deps.remove(&implied);
                }
            }
            // put new list back into the map
            deps.insert(*sys_id, sys_deps.into_iter().collect::<Vec<_>>());
        }

        // compute depth of systems
        while !self.systems.is_empty() {
            let sys_id = self.systems.remove(0);

            let deps = &deps[sys_id];
            if deps.is_empty() {
                // System has no dependency, its depths is 0
                depths.insert(sys_id, 0);
            } else {
                // Get the maximum depth of all the dependencies, or None if not all the
                // dependencies's depths are known.
                let max_depth = deps
                    .iter()
                    .map(|id| depths.get(*id).copied())
                    .reduce(|acc, item| acc.and_then(|acc| item.map(|item| acc.max(item))))
                    .unwrap();
                match max_depth {
                    // if we have a max, add one and set that as the depth
                    Some(m) => {
                        depths.insert(sys_id, m + 1);
                    }
                    // if we don't, put back the system into the array to try again later
                    None => {
                        self.systems.push(sys_id);
                    }
                }
            }
        }

        let mut depths = depths.into_iter().collect::<Vec<_>>();
        depths.sort_by_key(|v| v.1);
        // Get the systems sorted by depth
        let systems = depths.into_iter().map(|v| v.0).collect::<Vec<_>>();

        let mut threads: Vec<Vec<Step>> = Vec::new();
        let mut waits: Vec<Wait> = Vec::new();

        for sys in systems {
            let deps = deps[sys].iter().copied().collect::<HashSet<_>>();

            // If a suitable thread has been found
            let mut found = false;
            // The index of the thread the Run has been put
            let mut step_thread = 0usize;
            // The index of the step the run is in the thread
            let mut step_index = 0usize;

            'outer: for dep in deps.clone() {
                for (i, steps) in threads.iter_mut().enumerate() {
                    let last_run = steps
                        .iter()
                        .filter_map(|step| {
                            if let Step::Run(sys) = step {
                                Some(sys)
                            } else {
                                None
                            }
                        })
                        .last()
                        .copied()
                        .unwrap(); // threads have always atleast one Step::Run(...)
                    if last_run == dep {
                        // thread is suitable
                        found = true;
                        step_thread = i;
                        step_index = steps.len();
                        steps.push(Step::Run(sys));
                        break 'outer;
                    }
                }
            }
            // No suitable thread found
            if !found {
                step_thread = threads.len();
                step_index = 0;
                threads.push(vec![Step::Run(sys)]);
            }

            // Here we have placed the Run at index <index> of thread <thread>, we now need to
            // ensure that all dependencies are satisfied through syncronizations steps.

            for dep in deps {
                // Check the current thread for the dependency
                let in_thread = threads[step_thread].contains(&Step::Run(dep));

                if !in_thread {
                    // then sync is needed
                    // loop over the threads looking for the one that contains the dependency
                    for (thread_index, mut dep_thread) in threads.iter_mut().enumerate() {
                        let index = dep_thread.iter().position(|step| {
                            if let Step::Run(s) = step {
                                *s == dep
                            } else {
                                false
                            }
                        });
                        if let Some(index) = index {
                            let wait = {
                                // If there is already a wait before the run, then this is its
                                // index
                                let wait_index = step_index.saturating_sub(1);
                                let wait;

                                if let Step::Wait(w) = threads[step_thread][wait_index] {
                                    let new_limit = waits[w].limit() + 1;
                                    waits[w].set_limit(new_limit);
                                    wait = w;
                                } else {
                                    // there is no wait, we add one
                                    let w = Wait::new(1);
                                    wait = waits.len();
                                    waits.push(w);
                                    threads[step_thread].insert(step_index, Step::Wait(wait));
                                    step_index += 1;
                                }

                                dep_thread = &mut threads[thread_index];
                                wait
                            };

                            dep_thread.insert(index + 1, Step::Notify(wait));
                            break;
                        }
                    }
                }
            }
        }
        Schedule {
            executor_id: self.executor.id,
            threads: Arc::new(threads),
            waits: Arc::new(waits),
        }
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(PartialEq, Clone, Copy)]
enum Step {
    /// Run a system
    Run(SystemId),
    /// Notify another thread (for sync)
    Notify(usize),
    /// Wait for notifications
    Wait(usize),
}

pub struct Schedule {
    executor_id: ExecutorId,
    threads: Arc<Vec<Vec<Step>>>,
    waits: Arc<Vec<Wait>>,
}
