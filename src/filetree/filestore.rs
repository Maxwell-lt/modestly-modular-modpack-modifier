use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use xxhash_rust::xxh3::xxh3_128;

/// Content-addressed store of byte arrays (files).
#[derive(Debug, Clone)]
pub(crate) struct FileStore {
    data: Arc<RwLock<HashMap<u128, Arc<Vec<u8>>>>>,
}

impl FileStore {
    pub(crate) fn new() -> FileStore {
        FileStore {
            data: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Retrieve file from store by its hash.
    ///
    /// Locks the internal store for reading.
    pub(crate) fn get_file(&self, hash: u128) -> Option<Arc<Vec<u8>>> {
        let map = self.data.read();
        map.get(&hash).map(|a| a.clone())
    }

    /// Insert file into the store and get its hash.
    ///
    /// Locks the internal store for writing.
    pub(crate) fn write_file(&self, file: Vec<u8>) -> u128 {
        let hash = xxh3_128(file.as_slice());
        let mut map = self.data.write();
        map.insert(hash, Arc::new(file));
        hash
    }

    /// Retrieve set of files from store by a list of hashes.
    ///
    /// The returned array preserves the order of provided hash list.
    ///
    /// Returns [`None`] if any of the hashes are not found in the store.
    ///
    /// Locks the internal store for reading.
    pub(crate) fn get_all_files(&self, hashes: Vec<u128>) -> Option<Vec<Arc<Vec<u8>>>> {
        let map = self.data.read();
        hashes.iter().map(|hash| map.get(hash).map(|a| a.clone())).collect()
    }

    /// Store set of files and get a hash for each.
    ///
    /// The returned array preserves the order of the provided file list.
    ///
    /// Locks the internal store for writing.
    pub(crate) fn write_all_files(&self, files: Vec<Vec<u8>>) -> Vec<u128> {
        let hashes: Vec<u128> = files.iter().map(|f| xxh3_128(f)).collect();
        let mut map = self.data.write();
        for (file, hash) in files.into_iter().zip(hashes.iter()) {
            map.insert(*hash, Arc::new(file));
        }
        hashes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::thread;
    use tokio::sync::broadcast;

    #[test]
    fn store_and_retrieve_file() {
        let store = FileStore::new();
        let file_content = "Hello World\n";
        let file: Vec<u8> = file_content.to_string().into_bytes();

        let hash = store.write_file(file.clone());
        let retrieved_file: Arc<Vec<u8>> = store.get_file(hash).unwrap();

        assert_eq!(*retrieved_file, file);
    }

    #[test]
    fn store_and_retrieve_file_batch() {
        let store = FileStore::new();
        let file_1 = "Hello World!\n".to_string().into_bytes();
        let file_2 = "Multiline\nFile!\n".to_string().into_bytes();
        let file_3: Vec<u8> = vec![0, 40, 90, 255, 3, 52, 44, 128, 3];

        let hashes = store.write_all_files(vec![file_1.clone(), file_2.clone(), file_3.clone()]);
        let retrieved_files = store.get_all_files(hashes).unwrap();

        assert_eq!(*retrieved_files[0], file_1);
        assert_eq!(*retrieved_files[1], file_2);
        assert_eq!(*retrieved_files[2], file_3);
    }

    #[test]
    fn attempt_retrieve_with_invalid_hashes() {
        let store = FileStore::new();
        let file = "Hello World!\n".to_string().into_bytes();

        let valid_hash = store.write_file(file);

        let result = store.get_all_files(vec![valid_hash, 12345678]);

        assert!(result.is_none());
    }

    #[test]
    fn store_file_retrieve_in_threads() {
        // Create store and sync objects
        let store = FileStore::new();
        let (broadcast_tx, _) = broadcast::channel::<u128>(1);
        let counter = Arc::new(AtomicU8::new(0));
        // Spawn threads
        let mut handles = Vec::with_capacity(10);
        for _ in 0..10 {
            // Clone sync objects
            let mut rx = broadcast_tx.subscribe();
            let c = counter.clone();
            let s = store.clone();
            handles.push(thread::spawn(move || {
                // Get hash of file to try retrieving
                let hash = rx.blocking_recv().unwrap();
                let success = s.get_file(hash).is_some();
                if success {
                    // If file was retrieved successfully, increment counter
                    c.fetch_add(1, Ordering::Relaxed);
                }
            }))
        }

        // Add file to store
        let file = "Hello World!\n".to_string().into_bytes();
        let hash = store.write_file(file);

        // Tell threads to try reading file
        broadcast_tx.send(hash).unwrap();
        // Join all threads
        for handle in handles.into_iter() {
            handle.join().unwrap();
        }

        // Check that all threads successfully found the file
        assert_eq!(10, counter.load(Ordering::SeqCst));
    }
}
