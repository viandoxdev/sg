use std::{
    any::{Any, TypeId},
    collections::{HashMap, HashSet},
    error::Error,
    hash::Hash,
    marker::PhantomPinned,
    ops::Deref,
    sync::{atomic::AtomicU32, Arc},
    thread::JoinHandle,
};

use parking_lot::{Condvar, Mutex, RwLock, RwLockReadGuard};
use slotmap::{DefaultKey, SlotMap};

use crate::{
    bitset::{BitsetBuilder, BorrowBitset, BorrowBitsetBuilder, BorrowBitsetMapping},
    borrows::{BorrowGuard, Borrows},
    query::{Query, QueryIterBundle},
    system::{IntoSystem, RequirementsMappings, System, SystemId},
    thread_pool::{Job, ThreadPool},
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
    waits: ReadOnly<Vec<Wait>>,
    // TODO: remove 'static, as the context isn't static at all, but I can't have the lifetime on
    // the struct.
    context: ExecutionContext<'static>,
}

impl Job for ExecutorJob {
    fn execute(self) {
        for step in self.steps {
            match step {
                Step::Wait(index) => {
                    self.waits.get()[index].wait();
                }
                Step::Notify(index) => {
                    self.waits.get()[index].notify();
                }
                Step::Run(id) => {
                    self.context
                        .executor
                        .get_system(id)
                        .unwrap()
                        .run(&self.context);
                }
            }
        }
    }
}

/// A struct holding systems and resources
pub struct Executor {
    resources: HashMap<TypeId, Box<dyn Any>>,
    systems: HashMap<SystemId, System>,
    mappings: RequirementsMappings,
    thread_pool: ThreadPool<ExecutorJob>,
}

impl Executor {
    pub fn new() -> Self {
        Self {
            resources: HashMap::new(),
            systems: HashMap::new(),
            mappings: RequirementsMappings::new(),
            thread_pool: ThreadPool::new(),
        }
    }

    pub fn add_resource<T: Any>(&mut self, res: T) {
        if self.resources.contains_key(&TypeId::of::<T>()) {
            panic!(
                "Trying to add resource that is already in executor: {}",
                std::any::type_name::<T>()
            );
        }
        self.resources.insert(res.type_id(), Box::new(res));
    }

    pub fn get_resource<T: 'static>(&self) -> Option<&T> {
        self.resources
            .get(&TypeId::of::<T>())
            .map(|boxed| boxed.downcast_ref::<T>())
            .flatten()
    }
    /// Get a mutable reference to a resource without any checks for aliasing.
    pub unsafe fn get_resource_mut_unchecked<T: 'static>(&self) -> Option<&mut T> {
        let s = &mut *(self as *const Self as *mut Self);
        s.resources
            .get_mut(&TypeId::of::<T>())
            .map(|boxed| boxed.downcast_mut::<T>())
            .flatten()
    }
    pub fn get_resource_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.resources
            .get_mut(&TypeId::of::<T>())
            .map(|boxed| boxed.downcast_mut::<T>())
            .flatten()
    }
    /// Get a scheduler used to build a schedule
    pub fn schedule(&mut self) -> Scheduler {
        Scheduler {
            executor: self,
            systems: Vec::new(),
        }
    }

    pub fn has_system(&self, sys: impl IntoSystem) -> bool {
        self.systems.contains_key(&sys.id())
    }
    pub fn add_system(&mut self, sys: impl IntoSystem) {
        self.systems
            .insert(sys.id(), sys.into_system(&mut self.mappings));
    }
    fn get_system(&self, sys: SystemId) -> Option<&System> {
        self.systems.get(&sys)
    }
    /// Run a given schedule against this executor and a world
    pub fn execute(&mut self, schedule: &Schedule, world: &mut World) {
        let context = ExecutionContext {
            executor: self,
            world,
        };

        let waits = schedule.waits.clone();
        for thread in &*schedule.threads.get() {
            let job = ExecutorJob {
                waits: waits.clone(),
                context: unsafe { std::mem::transmute(context) },
                steps: thread.to_vec(),
            };

            self.thread_pool.run(job);
        }
    }
}

pub struct Scheduler<'a> {
    executor: &'a mut Executor,
    systems: Vec<SystemId>,
}

impl<'a> Scheduler<'a> {
    /// Add a system to the building schedule, a schedule can't contain the same system twice.
    pub fn then(mut self, sys: impl IntoSystem) -> Self {
        if !self.executor.has_system(sys) {
            self.executor.add_system(sys)
        }
        if !self.systems.contains(&sys.id()) {
            self.systems.push(sys.id());
            self
        } else {
            panic!("Trying to add the same system to a schedule twice. This isn't currently supported.");
        }
    }
    /// /!\ Fairely expensive, and optimized, should only be called a few times
    /// Create a schedule from the added systems, the schedule is parallelized as much as possible
    /// while keeping the same behaviour as if the systems were run sequentially.
    pub fn build(mut self) -> Schedule {
        if self.systems.is_empty() {
            return Schedule {
                threads: ReadOnly::new(Vec::new()),
                waits: ReadOnly::new(Vec::new()),
            };
        }

        let mut deps: HashMap<SystemId, Vec<SystemId>> = HashMap::new();
        let mut depths: HashMap<SystemId, u32> = HashMap::new();

        // find dependencies between systems
        for (i, sys_id) in self.systems.iter().enumerate() {
            let sys = self.executor.get_system(*sys_id).unwrap();
            // Loop over all the systems that come before
            for other_id in &self.systems[0..i] {
                let other = self.executor.get_system(*other_id).unwrap();

                if sys.depends_on(other) {
                    deps.entry(*sys_id)
                        .or_insert_with(|| Vec::new())
                        .push(*other_id)
                }
            }
        }
        // remove implicit dependencies
        for sys_id in &self.systems {
            // Take the dependencies from the map (and convert to set)
            let mut sys_deps = deps
                .remove(sys_id)
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
                    deps: &HashMap<SystemId, Vec<SystemId>>,
                ) {
                    for dep in &deps[&id] {
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

            let deps = &deps[&sys_id];
            if deps.is_empty() {
                // System has no dependency, its depths is 0
                depths.insert(sys_id, 0);
            } else {
                // Get the maximum depth of all the dependencies, or None if not all the
                // dependencies's depths are known.
                let max_depth = deps
                    .iter()
                    .map(|id| depths.get(&id).copied())
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
            let deps = deps[&sys].iter().copied().collect::<HashSet<_>>();

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
                                drop(dep_thread);
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
            threads: ReadOnly::new(threads),
            waits: ReadOnly::new(waits),
        }
    }
}

struct ReadOnly<T> {
    inner: Arc<RwLock<T>>,
}
// Manual impl of clone because derive doesn't understand that Arc<...> can be clone without ...
// being.
impl<T> Clone for ReadOnly<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

pub struct ReadOnlyGuard<'a, T>(RwLockReadGuard<'a, T>);

impl<'a, T> Deref for ReadOnlyGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &*self.0
    }
}

impl<T> ReadOnly<T> {
    fn new(val: T) -> Self {
        Self {
            inner: Arc::new(RwLock::new(val)),
        }
    }

    fn get(&self) -> ReadOnlyGuard<'_, T> {
        ReadOnlyGuard(self.inner.read())
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

/// A barrier like syncronizations struct, waits for a ceratin number of notifications.
pub struct Wait {
    cond: Condvar,
    count: Mutex<u32>,
    limit: AtomicU32,
}

impl Wait {
    /// Get the number of notifications before the wait ends
    fn limit(&self) -> u32 {
        self.limit.load(std::sync::atomic::Ordering::Relaxed)
    }
    /// Create a new Wait
    fn new(limit: u32) -> Self {
        Self {
            cond: Condvar::new(),
            count: Mutex::new(0),
            limit: AtomicU32::new(limit),
        }
    }
    /// Reset the counter
    fn reset(&self) {
        *self.count.lock() = 0;
    }
    /// Notify one
    fn notify(&self) {
        let mut count = self.count.lock();
        *count += 1;
        if *count == self.limit() {
            *count = 0;
            // release the lock
            drop(count);

            self.cond.notify_all();
        }
    }
    /// Change the limit of the Wait
    fn set_limit(&self, limit: u32) {
        self.limit.store(limit, std::sync::atomic::Ordering::SeqCst);
    }
    /// Wait for limit notifications
    fn wait(&self) {
        self.cond.wait(&mut self.count.lock());
    }
}

pub struct Schedule {
    threads: ReadOnly<Vec<Vec<Step>>>,
    waits: ReadOnly<Vec<Wait>>,
}

// UPDATE
// systems: rust functions
// traits:
//  - IntoSystem<0..16> => fn(...) -> ?
//     builds a System from a rust function, the function must have
//     from 0 to 16 (extended with features) arguments. The arguments must implement
//     SystemArgument.
//  - SystemArgument => Entities<...>, Res/ResMut<...>
//     represents a type that can be fetched from a Context
// structs:
//  - Entities<Q: Query> { ... }
//     a wrapper (or type alias) over a guard of an iterator (or whatever is returned by the world
//     when running a query).
//  - Res<R> / ResMut<R> { ... }
//     a wrapper around a reference to a resource, implements SystemArgument, only really here to
//     be explicit about if an argument is supposed to be a resource or not (Is it really necessary
//     when Entities exist ? since Entities is needed, can't we just assume that a reference is one
//     to a resource ?)
//  - System { requirements, pointer }
//     represents a system, holds no type information, allows the  system to be run through
//     the pointer, by calling run(context).
//  - Context { world, executor }
//     holds a reference to both to be able to fetch data from them (resources and entities)
//  - SystemPointer { system, run }
//     contains a fn pointer to the system's function, and a fn pointer to a function that
//     fetches what the system needs from a Context and passes it to the system's function
// concepts:
//  - A schedule runs a set of systems in an order, while using paralelization as much as possible
//    Only one schedule can be run at a time, many can be built and run against an executor and a
//    world. More advanced logic (run conditions in bevy) like certain frequency and state are to
//    be handled by the user (i.e every frame run the "render schedule", and 30 times a second run the
//    "physics" schedule).
//    For implmentation details: running a schedule is blocking and borrows the schedule and the
//    world (being run by the executor so borrows it as well).

#[cfg(test)]
mod tests {
    #[test]
    fn basic_system() {}
}

// TODO: Refractor most of this, currently lots of unsafe, annoying and outright dumb code that is
// hard to read. The plan stays the same. Currently missing things: Blocking for the eentire
// execution, maybe adding a notify at the end of all threads that notify a special wait ?
