use std::collections::HashMap;
use std::u128;
use std::sync::Arc;

use anyhow::{Result, bail};

use super::filestore::FileStore;
use super::filepath::FilePath;

#[derive(Clone)]
pub(crate) struct FileTree {
    contents: HashMap<String, u128>,
    store: FileStore,
}

impl FileTree {
    pub(crate) fn new(store: FileStore) -> FileTree {
        FileTree { contents: HashMap::new(), store }
    }

    pub(crate) fn add_file(&mut self, path: &FilePath, file: Vec<u8>) -> () {
        let hash = self.store.write_file(file);
        self.contents.insert(path.to_string(), hash);
    }

    pub(crate) fn get_file(&self, path: &FilePath) -> Option<Arc<Vec<u8>>> {
        if let Some(hash) = self.contents.get(&path.to_string()) {
            if let Some(file) = self.store.get_file(*hash) {
                return Some(file);
            }
        }
        return None;
    }
}
