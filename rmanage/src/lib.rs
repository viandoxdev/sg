#![feature(once_cell)]

use bimap::BiHashMap;
use directories::BaseDirs;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use slotmap::{SecondaryMap, SlotMap};
use std::{
    collections::HashMap,
    fs::File,
    hash::Hash,
    lazy::SyncOnceCell,
    path::{Path, PathBuf},
    sync::Arc,
};
use thiserror::Error;
use twox_hash::{xxh3::HasherExt, Xxh3Hash128};

slotmap::new_key_type! {
    pub struct Resource;
}

#[derive(Debug, Error)]
pub enum ResourceError {
    #[error("The global resourceManager has already been initialized")]
    AlreadyInitialized,
    #[error("An IO Error has occured: {0}")]
    IOError(std::io::Error),
    #[error("The resource already has a relation of this type")]
    WouldOverwriteRelation,
    #[error("The resource is virtual")]
    ResourceIsVirtual,
    #[error("The resource doesn't exist")]
    NoSuchResource,
    #[error("The resource manager doesn't have a cache path")]
    NoCachePath,
    #[error("A Bincode error occured on (de)serialization: {0}")]
    BinCodeError(bincode::ErrorKind),
}

impl From<std::io::Error> for ResourceError {
    fn from(e: std::io::Error) -> Self {
        Self::IOError(e)
    }
}

impl From<Box<bincode::ErrorKind>> for ResourceError {
    fn from(e: Box<bincode::ErrorKind>) -> Self {
        Self::BinCodeError(*e)
    }
}

#[derive(Deserialize, Serialize, Debug)]
struct RawResourceManager {
    #[serde(skip)]
    resources_data: SecondaryMap<Resource, Option<Arc<[u8]>>>,
    // Annoying but necessary as there is no other way to keep the same keys otherwise
    resources: SlotMap<Resource, ()>,
    relations: HashMap<(Resource, String), Resource>,
    locations: BiHashMap<PathBuf, Resource>,
    virtual_resources: SecondaryMap<Resource, ()>,
}

#[derive(Serialize, Deserialize, Debug)]
struct PhysicalResource {
    path: PathBuf,
    size: usize,
    hash: u128,
}

#[derive(Serialize, Deserialize, Debug)]
struct PhysicalResourcesMeta {
    physical_resources: SecondaryMap<Resource, PhysicalResource>,
    seed: u64,
}

pub struct ResourceManager {
    resources_path: PathBuf,
    cache_path: Option<PathBuf>,
    raw: RwLock<RawResourceManager>,
}

fn mkdir(path: impl AsRef<Path>) {
    if let Err(e) = std::fs::create_dir_all(path) {
        if e.kind() != std::io::ErrorKind::AlreadyExists {
            panic!("Error on directory creation: {e}");
        }
    }
}

impl ResourceManager {
    /// Get the ResourceManager's resource directory
    pub fn directory(&self) -> &Path {
        self.resources_path.as_path()
    }
    /// Add a physical resource (linked to a file).
    /// Relative paths are resolved from the resources directory (see `ResourceManager::directory`)
    ///
    /// Calling this multiple time with the same path will return the same resource. It is faster
    /// to keep the `Resource` handle arround than to keep querying it from here.
    ///
    /// # Note
    ///
    /// This may be innacurate if the content of the file has been changed when the file has
    /// already been loaded (subsequent loads will act as if the data is still valid when it
    /// isn't). There is currently no way around that.
    pub fn add_physical(&self, path: impl AsRef<Path>) -> Result<Resource, ResourceError> {
        let path = path.as_ref();
        let path = if path.is_relative() {
            self.resources_path.join(path).canonicalize()?
        } else {
            path.to_path_buf().canonicalize()?
        };

        if let Some(res) = self.raw.read().locations.get_by_left(&path) {
            return Ok(*res);
        }

        let mut raw = self.raw.write();
        let res = raw.resources.insert(());
        raw.resources_data.insert(res, None);
        raw.locations.insert(path, res);
        Ok(res)
    }
    /// Create a virtual resource with the associated data.
    /// There is no way to access the created resource without its `Resource` handle (if no
    /// relation point to it), and dropping it will effectively be a memory leak.
    pub fn add_virtual(&self, data: &[u8]) -> Resource {
        let mut raw = self.raw.write();
        let res = raw.resources.insert(());
        raw.resources_data.insert(res, Some(Arc::from(data)));
        raw.virtual_resources.insert(res, ());
        res
    }
    /// Set the relation between two resources. A relation between two resources implies that one
    /// is derived from another.
    /// Currently relations are one to one and directed, a resource can only have one relation of a
    /// kind that points to a single other resource. Setting a relation `R` from `a` to `b` will nothing
    /// change the relation from `b` to `a`.
    pub fn set_relation(
        &self,
        relation: &str,
        from: Resource,
        to: Resource,
    ) -> Result<(), ResourceError> {
        let key = (from, relation.to_owned());
        if self.raw.read().relations.contains_key(&key) {
            Err(ResourceError::WouldOverwriteRelation)
        } else {
            self.raw.write().relations.insert(key, to);
            Ok(())
        }
    }
    /// Ensure a physical resource is in ram. Physical resources are lazy loaded.
    pub fn ensure_loaded(&self, res: Resource) -> Result<(), ResourceError> {
        let unloaded = self
            .raw
            .read()
            .resources_data
            .get(res)
            .ok_or(ResourceError::NoSuchResource)?
            .is_none();

        if unloaded {
            let raw = self.raw.read();
            // If resource is physical
            if let Some(loc) = raw.locations.get_by_right(&res) {
                let bytes = Arc::from(std::fs::read(loc)?.into_boxed_slice());

                drop(raw);
                self.raw
                    .write()
                    .resources_data
                    .get_mut(res)
                    .unwrap()
                    .replace(bytes);
            }
            Ok(())
        } else {
            Ok(())
        }
    }
    /// Get a resource's data. This may block for IO if the resource isn't already loaded.
    /// A resource can be preloaded witth `ResourceManager::ensure_loaded`.
    pub fn get_resource(&self, res: Resource) -> Result<Arc<[u8]>, ResourceError> {
        self.ensure_loaded(res)?;
        Ok(self
            .raw
            .read()
            .resources_data
            .get(res)
            .unwrap() // Alright because ensure loaded would have returned an error already
            .as_ref()
            .unwrap() // Alright because thats what ensure loaded guarentees
            .clone())
    }
    /// Get a related resource
    pub fn get_related(&self, res: Resource, relation: &str) -> Option<Resource> {
        self.raw
            .read()
            .relations
            .get(&(res, relation.to_owned()))
            .copied()
    }
    /// Returns true if the ResourceManager contains the resource
    pub fn contains(&self, res: Resource) -> bool {
        self.raw.read().resources.contains_key(res)
    }
    /// Returns true if the ResourceManager contains the resource, and if it is virtual
    pub fn contains_virtual(&self, res: Resource) -> bool {
        self.raw.read().virtual_resources.contains_key(res)
    }
    /// Returns true if the ResourceManager contains the resource, and if it is physical
    pub fn contains_physical(&self, res: Resource) -> bool {
        self.raw.read().locations.contains_right(&res)
    }
    /// Free a physical resource. This doesn't delete it, but simply removes it from ram.
    /// Calling `ResourceManager::get_resource` on a freed resource will result in a blocking read.
    ///
    /// This is meaningless for virtual (not loaded from files) resources as once freed there is no
    /// way to recover one, so this returns an `Err(ResourceError::ResourceIsVirtual)` in that
    /// case.
    ///
    /// Calling this on a freed resource does nothing and returns `Ok(())`.
    pub fn free(&self, res: Resource) -> Result<(), ResourceError> {
        if !self.contains(res) {
            return Err(ResourceError::NoSuchResource);
        }
        if self.contains_virtual(res) {
            Err(ResourceError::ResourceIsVirtual)
        } else {
            self.raw.write().resources_data.get_mut(res).unwrap().take();
            Ok(())
        }
    }
    /// Write virtual resources to cache if the cache directory is set.
    pub fn cache(&self) -> Result<(), ResourceError> {
        let cache_path = self.cache_path.as_ref().ok_or(ResourceError::NoCachePath)?;
        let mut meta = PhysicalResourcesMeta {
            seed: rand::random(),
            physical_resources: SecondaryMap::new(),
        };

        let locations = self
            .raw
            .read()
            .locations
            .iter()
            .map(|(p, r)| (p.clone(), *r))
            .collect::<Vec<_>>(); // Necessary to release self.raw

        for (path, res) in locations {
            let data = self.get_resource(res)?;
            let size = data.len();
            let mut hasher = Xxh3Hash128::with_seed(meta.seed);
            data.hash(&mut hasher);
            let hash = hasher.finish_ext();

            meta.physical_resources
                .insert(res, PhysicalResource { path, size, hash });
        }

        for res in self.raw.read().resources.keys() {
            if !meta.physical_resources.contains_key(res) {
                let name = res.0.as_ffi().to_string();
                let data = self.get_resource(res)?;
                let path = cache_path.join(name);
                std::fs::write(path, &data)?;
            }
        }

        let cache = bincode::serialize(&*self.raw.read())?;
        let meta = bincode::serialize(&meta)?;
        std::fs::write(cache_path.join("cache"), &cache)?;
        std::fs::write(cache_path.join("meta"), &meta)?;
        Ok(())
    }
    /// This tries to read the cache and get virtual resources from it. This overrides any
    /// resources previously put. This should be called at the start of the application, but can be
    /// called anytime as long as the side effects are handled.
    pub fn sync_cache(&self) -> Result<(), ResourceError> {
        let cache_path = self.cache_path.as_ref().ok_or(ResourceError::NoCachePath)?;
        let cache_file = File::open(cache_path.join("cache"))?;
        let meta_file = File::open(cache_path.join("meta"))?;

        let mut cache: RawResourceManager = bincode::deserialize_from(cache_file)?;
        let mut meta: PhysicalResourcesMeta = bincode::deserialize_from(meta_file)?;

        // Remove dead physical resources. A physical resource is dead if the file at the
        // resource's path doesn't match in size of hash with the resource
        meta.physical_resources.retain(|res, info| {
            let retain = std::fs::read(&info.path)
                .map(|buf| {
                    let size = buf.len();
                    let mut hasher = Xxh3Hash128::with_seed(meta.seed);
                    buf.hash(&mut hasher);
                    let hash = hasher.finish_ext();

                    size == info.size && hash == info.hash
                })
                .unwrap_or_default();

            if !retain {
                cache.locations.remove_by_right(&res);
            }

            retain
        });

        // Remove dead virtual resources, virtual resources that are related to dead physical ones,
        // either directly or indirectly.
        let mut delta = 1;
        while delta > 0 {
            delta = 0;
            cache.relations.retain(|(from, _), to| {
                let kill = cache.virtual_resources.contains_key(*to)
                    && !(cache.virtual_resources.contains_key(*from)
                        || cache.locations.contains_right(from));
                if kill {
                    delta += 1;
                    cache.virtual_resources.remove(*to);
                    let filename = to.0.as_ffi().to_string();
                    std::fs::remove_file(cache_path.join(filename)).ok();
                }
                !kill // remove the relation if both resources are dead
            });
        }

        // Load the virtual resource's data and remove the keys of dead resources
        cache.resources.retain(|res, _| {
            let value = if cache.virtual_resources.contains_key(res) {
                // Resource is virtual: we load it from it's cache file
                //
                let filename = res.0.as_ffi().to_string();
                // Not a fan of the unwrap here
                let bytes = std::fs::read(cache_path.join(filename)).unwrap();
                Some(Arc::from(bytes.into_boxed_slice()))
            } else if meta.physical_resources.contains_key(res) {
                // Resource is physical: we lazy load it
                None
            } else {
                // Resource is neither virtual or physical: it's dead
                return false;
            };
            cache.resources_data.insert(res, value);
            true
        });

        *self.raw.write() = cache;

        Ok(())
    }
}

impl Default for RawResourceManager {
    fn default() -> Self {
        Self {
            resources: SlotMap::with_key(),
            relations: HashMap::new(),
            locations: BiHashMap::new(),
            virtual_resources: SecondaryMap::new(),
            resources_data: SecondaryMap::new(),
        }
    }
}

#[derive(Default)]
pub struct ResourceManagerBuilder {
    res_path: Option<PathBuf>,
    cache_path: Option<PathBuf>,
}

impl ResourceManagerBuilder {
    /// Create a new ResourceManagerBuilder with default values
    pub fn begin() -> Self {
        Self::default()
    }
    /// Add a cache directory to the ResourceManager (path will be in the user's cache directory)
    pub fn with_cache(mut self, app_name: Option<&str>) -> Self {
        self.cache_path = BaseDirs::new().and_then(|d| {
            let res = match app_name {
                Some(name) => d.cache_dir().join(name).join("resources_cache"),
                None => d.cache_dir().join(".resources_cache"),
            }
            .canonicalize()
            .ok()?;
            mkdir(&res);
            Some(res)
        });
        self
    }
    /// Add a cache directory to the ResourceManager
    pub fn with_cache_path(mut self, path: impl AsRef<Path>) -> Self {
        self.cache_path = Some(path.as_ref().to_owned());
        self
    }
    /// Set the resources directory path
    pub fn with_resource_path(mut self, path: impl AsRef<Path>) -> Self {
        self.res_path = Some(path.as_ref().to_owned());
        self
    }
    /// Build the ResourceManager
    pub fn build(self) -> ResourceManager {
        let resources_path = self
            .res_path
            .unwrap_or_else(|| PathBuf::from("./resources"));
        let cache_path = self.cache_path;
        mkdir(&resources_path);
        let resources_path = resources_path.canonicalize().unwrap();

        ResourceManager {
            resources_path,
            cache_path,
            raw: Default::default(),
        }
    }
}

static RESOURCE_MANAGER: SyncOnceCell<ResourceManager> = SyncOnceCell::new();

/// initialize the resource manager. Must be called before `instance`, ideally at the begining of
/// main.
pub fn init(builder: ResourceManagerBuilder) -> Result<(), ResourceError> {
    RESOURCE_MANAGER
        .set(builder.build())
        .map_err(|_| ResourceError::AlreadyInitialized)
}
pub fn init_default() -> Result<(), ResourceError> {
    init(ResourceManagerBuilder::begin())
}
/// Get the global instance of the resource manager.
///
/// # Panics
///
/// This panics if the resource manager hasn't been initialized. To initialize it call `init` or
/// `init_default`
pub fn instance() -> &'static ResourceManager {
    RESOURCE_MANAGER
        .get()
        .expect("ResourceManager hasn't been initialized")
}

#[cfg(test)]
mod tests {
    use super::*;
    use mktemp::Temp;
    use std::ops::Deref;
    // Keep the temp directory alive
    struct G(ResourceManager, Temp, Temp);
    impl Deref for G {
        type Target = ResourceManager;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }
    fn _init() -> G {
        let temp = Temp::new_dir().unwrap();
        let cache = Temp::new_dir().unwrap();
        let rm = ResourceManagerBuilder::begin()
            .with_resource_path(temp.as_path())
            .with_cache_path(cache.as_path())
            .build();
        G(rm, temp, cache)
    }

    const UPPERCASE: &str = "UPPERCASE";
    const LOWERCASE: &str = "LOWERCASE";

    #[test]
    fn init() {
        let _rm = _init();
    }

    #[test]
    fn file() {
        let rm = _init();
        let content = "Hey There!";
        let temp = Temp::new_file_in(rm.directory()).unwrap();
        std::fs::write(temp.as_path(), content.as_bytes()).unwrap();

        let res = rm.add_physical(temp.as_path()).unwrap();
        let res2 = rm.add_physical(temp.as_path()).unwrap();
        assert_eq!(res, res2);

        let bytes = rm.get_resource(res).unwrap();
        let s = std::str::from_utf8(&bytes).unwrap();
        assert_eq!(s, content);
    }

    #[test]
    fn virtual_relation() {
        let rm = _init();
        let content = "looooonnnnng loooonnnng maaaaannn";
        let temp = Temp::new_file_in(rm.directory()).unwrap();
        std::fs::write(temp.as_path(), content).unwrap();

        let p = rm.add_physical(temp.as_path()).unwrap();

        {
            let processed = rm
                .get_resource(p)
                .unwrap()
                .iter()
                .map(u8::to_ascii_uppercase)
                .collect::<Vec<_>>();

            let v = rm.add_virtual(&processed);
            rm.set_relation(UPPERCASE, p, v).unwrap();
        }

        let v = rm.get_related(p, UPPERCASE).unwrap();
        let pb = rm.get_resource(p).unwrap();
        let pb = std::str::from_utf8(&pb).unwrap();
        let vb = &rm.get_resource(v).unwrap();
        let vb = std::str::from_utf8(&vb).unwrap();

        assert_eq!(pb, content);
        assert_eq!(vb, content.to_uppercase());
    }

    #[test]
    fn cache() {
        let G(rm, res_temp, cache_temp) = _init();
        let temp;
        {
            let content = "This is a String!";
            temp = Temp::new_file_in(rm.directory()).unwrap();
            std::fs::write(temp.as_path(), &content).unwrap();
            let pr = rm.add_physical(temp.as_path()).unwrap();

            {
                let data = rm.get_resource(pr).unwrap();
                let v1 = data
                    .iter()
                    .map(|b| b.to_ascii_uppercase())
                    .collect::<Vec<_>>();
                let v2 = data
                    .iter()
                    .map(|b| b.to_ascii_lowercase())
                    .collect::<Vec<_>>();
                let v1 = rm.add_virtual(&v1);
                let v2 = rm.add_virtual(&v2);
                rm.set_relation(UPPERCASE, pr, v1).unwrap();
                rm.set_relation(LOWERCASE, pr, v2).unwrap();
            }

            rm.free(pr).unwrap();

            rm.cache().unwrap();
        }

        drop(rm);
        let rm = ResourceManagerBuilder::begin()
            .with_resource_path(res_temp.as_path())
            .with_cache_path(cache_temp.as_path())
            .build();

        rm.sync_cache().unwrap();

        let pr = rm.add_physical(temp.as_path()).unwrap();
        let v1 = rm.get_related(pr, UPPERCASE).unwrap();
        let v2 = rm.get_related(pr, LOWERCASE).unwrap();

        let data = rm.get_resource(pr).unwrap();
        assert_eq!("This is a String!", std::str::from_utf8(&data).unwrap());

        let data = rm.get_resource(v1).unwrap();
        assert_eq!("THIS IS A STRING!", std::str::from_utf8(&data).unwrap());

        let data = rm.get_resource(v2).unwrap();
        assert_eq!("this is a string!", std::str::from_utf8(&data).unwrap());
    }
}
