use crate::{
    bitset::{BitsetBuilder, BorrowBitset, BorrowBitsetBuilder, BorrowBitsetMapping},
    borrows::BorrowGuard,
    query::{Query, QueryIterBundle},
    scheduler::ExecutionContext,
};
use ecs_macros::impl_system;
use std::any::TypeId;

pub struct Requirements {
    components: BorrowBitset,
    resources: BorrowBitset,
}

pub struct RequirementsMappings {
    components: BorrowBitsetMapping,
    resources: BorrowBitsetMapping,
}

impl RequirementsMappings {
    pub fn new() -> Self {
        Self {
            components: BorrowBitsetMapping::new(),
            resources: BorrowBitsetMapping::new(),
        }
    }
}

pub struct RequirementsBuilder<'a> {
    components: BorrowBitsetBuilder<'a>,
    resources: BorrowBitsetBuilder<'a>,
}

impl<'a> RequirementsBuilder<'a> {
    pub fn start(mappings: &'a RequirementsMappings) -> Self {
        Self {
            components: BorrowBitsetBuilder::start(&mappings.components),
            resources: BorrowBitsetBuilder::start(&mappings.resources),
        }
    }
    pub fn build(self) -> Option<Requirements> {
        let components = self.components.build()?;
        let resources = self.resources.build()?;
        Some(Requirements {
            components,
            resources,
        })
    }
}

#[derive(PartialEq, Eq, Hash, Clone, Copy)]
pub struct SystemId(usize);

pub trait IntoSystem: Copy {
    // IntoSystem is implemented for fn pointers which all implement copy
    fn run(self, context: &ExecutionContext);
    fn into_system(self, mappings: &mut RequirementsMappings) -> System;
    fn id(self) -> SystemId;
}

trait SystemArgument<'r> {
    fn fetch(context: &ExecutionContext) -> Self;
    fn require<'a>(builder: RequirementsBuilder) -> RequirementsBuilder;
    fn register(mappings: &mut RequirementsMappings);
}

pub type Entities<'a, Q> = BorrowGuard<'a, QueryIterBundle<Q>>;

impl<'r, Q: Query> SystemArgument<'r> for Entities<'r, Q> {
    fn register(mappings: &mut RequirementsMappings) {
        for ty in Q::types() {
            if !mappings.components.has(&ty) {
                mappings.components.map(ty);
            }
        }
    }
    fn require<'a>(mut builder: RequirementsBuilder) -> RequirementsBuilder {
        builder.components = Q::add_to_bitset(builder.components);
        builder
    }
    fn fetch(context: &ExecutionContext) -> Self {
        // SAFETY: ¯\_(ツ)_/¯ its probably alr
        unsafe { std::mem::transmute(context.world.query::<Q>()) }
    }
}

impl<'r, T: 'static> SystemArgument<'r> for &'r T {
    fn register(mappings: &mut RequirementsMappings) {
        if !mappings.resources.has(&TypeId::of::<T>()) {
            mappings.resources.map(TypeId::of::<T>());
        }
    }
    fn require<'a>(mut builder: RequirementsBuilder) -> RequirementsBuilder {
        builder.resources = builder.resources.borrow::<T>();
        builder
    }
    fn fetch(context: &ExecutionContext) -> Self {
        let res = context
            .executor
            .get_resource::<T>()
            .unwrap_or_else(|| panic!("Resource not in system: {}", std::any::type_name::<T>()));
        // transform lifetime to be valid. The system is guarenteed to be run with reference that
        // lives for at least the duration of the function's runtime.
        unsafe { &*(res as *const T) }
    }
}

impl<'r, T: 'static> SystemArgument<'r> for &'r mut T {
    fn register(mappings: &mut RequirementsMappings) {
        if !mappings.resources.has(&TypeId::of::<T>()) {
            mappings.resources.map(TypeId::of::<T>());
        }
    }
    fn require<'a>(mut builder: RequirementsBuilder) -> RequirementsBuilder {
        builder.resources = builder.resources.borrow::<T>();
        builder
    }
    fn fetch(context: &ExecutionContext) -> Self {
        unsafe {
            // Scheduling guarentees no aliasing
            // lives for at least the duration of the function's runtime.
            let res = context
                .executor
                .get_resource_mut_unchecked::<T>()
                .unwrap_or_else(|| {
                    panic!("Resource not in system: {}", std::any::type_name::<T>())
                });
            // transform lifetime to be valid. The system is guarenteed to be run with reference that
            &mut *(res as *mut T)
        }
    }
}

pub struct System {
    requirements: Requirements,
    pointer: SystemPointer,
}

impl System {
    pub fn depends_on(&self, other: &Self) -> bool {
        self.requirements
            .components
            .collide(other.requirements.components)
            || self
                .requirements
                .resources
                .collide(other.requirements.resources)
    }
    pub fn run(&self, context: &ExecutionContext) {
        self.pointer.execute(context);
    }
}

pub struct SystemPointer {
    /// A fn pointer to the system's function, this pointer isn't valid in itself as its arguments
    /// aren't known. It is unsafe to call this directly
    callee: fn(),
    /// A fn pointer to a function that will pass the right arguments to the callee by fetching
    /// them from an ExecutionContext.
    caller: fn(fn(), &ExecutionContext),
}

/// Akin to a fn pointer of a system
impl SystemPointer {
    #[inline(always)]
    pub fn execute(&self, context: &ExecutionContext) {
        (self.caller)(self.callee, context)
    }
}

#[cfg(not(feature = "extended_limits"))]
impl_system!(16);
#[cfg(feature = "extended_limits")]
impl_system!(24);
