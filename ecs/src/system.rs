use crate::{
    bitset::{BitsetBuilder, BorrowBitset, BorrowBitsetBuilder, BorrowBitsetMapping},
    executor::ExecutionContext,
    query::{Query, QueryIterBundle},
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

impl Default for RequirementsMappings {
    fn default() -> Self {
        Self::new()
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

/// A trait implemented on all Fn that are systems
pub trait IntoSystem<A> {
    /// Create a System struct representing the system
    fn into_system(self, mappings: &mut RequirementsMappings) -> System;
}

trait SystemArgument {
    /// Fetch the argument from an ExecutionContext, this ignores aliasing and is unsafe
    unsafe fn fetch(context: &ExecutionContext) -> Self;
    /// Get the requirements that this argument implies
    fn require(builder: RequirementsBuilder) -> RequirementsBuilder;
    /// Register the types that this argument references in the mapping
    fn register(mappings: &mut RequirementsMappings);
}

pub type Entities<Q> = QueryIterBundle<Q>;

impl<Q: Query> SystemArgument for Entities<Q> {
    fn register(mappings: &mut RequirementsMappings) {
        for ty in Q::types() {
            if !mappings.components.has(&ty) {
                mappings.components.map(ty);
            }
        }
    }
    fn require(mut builder: RequirementsBuilder) -> RequirementsBuilder {
        builder.components = Q::add_to_bitset(builder.components);
        builder
    }
    unsafe fn fetch(context: &ExecutionContext) -> Self {
        std::mem::transmute(context.world.query_unchecked::<Q>())
    }
}

impl<'r, T: 'static> SystemArgument for &'r T {
    fn register(mappings: &mut RequirementsMappings) {
        if !mappings.resources.has(&TypeId::of::<T>()) {
            mappings.resources.map(TypeId::of::<T>());
        }
    }
    fn require(mut builder: RequirementsBuilder) -> RequirementsBuilder {
        builder.resources = builder.resources.borrow::<T>();
        builder
    }
    unsafe fn fetch(context: &ExecutionContext) -> Self {
        let res = context
            .executor
            .get_resource::<T>()
            .unwrap_or_else(|| panic!("Resource not in system: {}", std::any::type_name::<T>()));
        // transform lifetime to be valid
        &*(res as *const T)
    }
}

impl<'r, T: 'static> SystemArgument for &'r mut T {
    fn register(mappings: &mut RequirementsMappings) {
        if !mappings.resources.has(&TypeId::of::<T>()) {
            mappings.resources.map(TypeId::of::<T>());
        }
    }
    fn require(mut builder: RequirementsBuilder) -> RequirementsBuilder {
        builder.resources = builder.resources.borrow::<T>();
        builder
    }
    unsafe fn fetch(context: &ExecutionContext) -> Self {
        let res = context
            .executor
            .get_resource_mut_unchecked::<T>()
            .unwrap_or_else(|| panic!("Resource not in system: {}", std::any::type_name::<T>()));
        // transform lifetime to be valid.
        &mut *(res as *mut T)
    }
}

/// A struct representing a system with some metadata
pub struct System {
    requirements: Requirements,
    run: Box<dyn Fn(&ExecutionContext)>,
}

impl System {
    /// Check if the system depends on another
    pub fn depends_on(&self, other: &Self) -> bool {
        self.requirements
            .components
            .collide(other.requirements.components)
            || self
                .requirements
                .resources
                .collide(other.requirements.resources)
    }
    /// Execute the system, this bypasses any aliasing checks and should only be used when proven
    /// safe
    pub unsafe fn run(&self, context: &ExecutionContext) {
        (self.run)(context);
    }
}

#[cfg(not(feature = "extended_limits"))]
impl_system!(16);
#[cfg(feature = "extended_limits")]
impl_system!(24);

// Annoyingly enough, this can't really be tested as is, because systems rely on an
// ExecutionContext and a Schedule guarenteeing safety.
