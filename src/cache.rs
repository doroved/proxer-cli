use lazy_static::lazy_static;
use std::{collections::HashMap, sync::Mutex};

// Store config_index and filter_index by host
#[derive(Debug)]
pub struct HostCache {
    pub cache: HashMap<String, (usize, usize)>,
}

impl HostCache {
    // Create a new HostCache instance
    pub fn new() -> HostCache {
        HostCache {
            cache: HashMap::new(),
        }
    }

    // Add host, config_index and filter_index to cache
    pub fn add(&mut self, host: String, config_index: usize, filter_index: usize) {
        self.cache.insert(host, (config_index, filter_index));
    }

    // Check if the host is in the cache
    pub fn contains(&self, host: &str) -> bool {
        self.cache.contains_key(host)
    }

    // Get config_index and filter_index by host
    pub fn get(&self, host: &str) -> Option<&(usize, usize)> {
        self.cache.get(host)
    }
}

// Create a static mutex for the HostCache
lazy_static! {
    pub static ref HOST_CACHE: Mutex<HostCache> = Mutex::new(HostCache::new());
}
