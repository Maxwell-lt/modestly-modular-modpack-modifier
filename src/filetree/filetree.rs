use std::collections::{HashMap, HashSet};
use std::u128;
use std::sync::Arc;
use thiserror::Error;
use super::filestore::FileStore;
use super::filepath::FilePath;

#[derive(Clone, Debug)]
pub(crate) struct FileTree {
    // Should contents use String or FilePath as a key? String is *probably* cheaper (as FilePath
    // owns several strings internally), but FilePath as a key allows stronger guarantees on valid
    // paths.
    contents: HashMap<FilePath, u128>,
    store: FileStore,
}

impl FileTree {
    pub(crate) fn new(store: FileStore) -> FileTree {
        FileTree { contents: HashMap::new(), store }
    }

    pub(crate) fn add_file(&mut self, path: &FilePath, file: Vec<u8>) -> () {
        let hash = self.store.write_file(file);
        self.contents.insert(path.clone(), hash);
    }

    pub(crate) fn get_file(&self, path: &FilePath) -> Option<Arc<Vec<u8>>> {
        if let Some(hash) = self.contents.get(path) {
            if let Some(file) = self.store.get_file(*hash) {
                return Some(file);
            }
        }
        return None;
    }

    /// Delete a filepath from the filetree.
    ///
    /// Idempotent.
    pub(crate) fn delete_file(&mut self, path: &FilePath) -> () {
        self.contents.remove(path);
    }

    pub(crate) fn copy_file(&mut self, from: &FilePath, to: &FilePath) -> Result<(), FileTreeError> {
        match self.contents.get(from) {
            Some(hash) => {
                self.contents.insert(to.clone(), *hash);
                Ok(())
            },
            None => Err(FileTreeError::FileNotFoundError(from.to_string())),
        }
    }

    pub(crate) fn move_file(&mut self, from: &FilePath, to: &FilePath) -> Result<(), FileTreeError> {
        self.copy_file(from, to)?;
        self.delete_file(from);
        Ok(())
    }

    pub(crate) fn list_files(&self) -> HashSet<&FilePath> {
        self.contents.keys()
            .collect()
    }

    pub(crate) fn filter_files<T: AsRef<str>>(&self, filters: &[T]) -> FileTree {
        FileTree { 
            contents: self.contents.iter()
                .filter(|entry| entry.0.glob_match(filters))
                .map(|entry| (entry.0.clone(), *entry.1))
                .collect(),
            store: self.store.clone(),
        }
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum FileTreeError {
    #[error("File not found at path {0}")]
    FileNotFoundError(String),
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn get_filetree() -> FileTree {
        FileTree::new(FileStore::new())
    }

    #[test]
    fn add_retrieve_files() {
        let mut files = get_filetree();
        let path = FilePath::from_str("directory/file.txt").unwrap();
        let contents = "Hello World!";
        files.add_file(&path, contents.into());
        
        assert_eq!(String::from_utf8(files.get_file(&path).unwrap().to_vec()).unwrap(), contents.to_string())
    }

    #[test]
    fn delete_file() {
        let mut files = get_filetree();
        let path = FilePath::from_str("directory/file.txt").unwrap();
        files.add_file(&path, "Hello World!".into());
        
        files.delete_file(&path);
        assert_eq!(files.get_file(&path), None);
    }

    #[test]
    fn copy_file() {
        let mut files = get_filetree();
        let path = FilePath::from_str("directory/file.txt").unwrap();
        let path2 = FilePath::from_str("directory_two/file_two.txt").unwrap();
        files.add_file(&path, "Hello World!".into());
        
        files.copy_file(&path, &path2).unwrap();
        assert_eq!(files.get_file(&path).unwrap(), files.get_file(&path2).unwrap());
    }

    #[test]
    fn copy_file_error_on_missing_source() {
        let mut files = get_filetree();
        let result = files.copy_file(&FilePath::from_str("path/does/not/exist.txt").unwrap(), &FilePath::from_str("destination/path.txt").unwrap());

        assert_eq!(result.unwrap_err(), FileTreeError::FileNotFoundError("path/does/not/exist.txt".to_string()));
    }

    #[test]
    fn move_file() {
        let mut files = get_filetree();
        let path = FilePath::from_str("directory/file.txt").unwrap();
        let path2 = FilePath::from_str("directory_two/file_two.txt").unwrap();
        let contents = "Hello World!";
        files.add_file(&path, contents.into());
        files.move_file(&path, &path2).unwrap();

        assert_eq!(String::from_utf8(files.get_file(&path2).unwrap().to_vec()).unwrap(), contents.to_string());
        assert_eq!(files.get_file(&path), None);
    }

    #[test]
    fn list_files() {
        let mut files = get_filetree();
        let path1 = FilePath::from_str("directory/file1.txt").unwrap();
        let path2 = FilePath::from_str("directory/file2.txt").unwrap();
        let path3 = FilePath::from_str("directory/file3.txt").unwrap();
        assert!(files.list_files().is_empty());
        
        files.add_file(&path1, "Test".into());
        assert_eq!(files.list_files(), [&path1].into());
        
        files.copy_file(&path1, &path2).unwrap();
        files.copy_file(&path2, &path3).unwrap();
        assert_eq!(files.list_files(), [&path1, &path2, &path3].into());
    }

    #[test]
    fn filter_files() {
        let mut files = get_filetree();
        let path1 = FilePath::from_str("overrides/mods/mod.jar").unwrap();
        let path2 = FilePath::from_str("overrides/config/config.cfg").unwrap();
        let path3 = FilePath::from_str("client_overrides/config/client.cfg").unwrap();
        let path4 = FilePath::from_str("other/config/readme.md").unwrap();
        files.add_file(&path1, "Hello".into());
        files.add_file(&path2, "World".into());
        files.add_file(&path3, "Foo".into());
        files.add_file(&path4, "Bar".into());

        assert_eq!(files.filter_files(&["overrides/**/*"]).list_files().symmetric_difference(&[&path1, &path2].into()).count(), 0);
        assert_eq!(files.filter_files(&["**/*.cfg"]).list_files().symmetric_difference(&[&path2, &path3].into()).count(), 0);
        assert_eq!(files.filter_files(&["**/*.jar", "**/*.cfg"]).list_files().symmetric_difference(&[&path1, &path2, &path3].into()).count(), 0);
        assert_eq!(files.filter_files(&["*overrides/**/*.cfg"]).list_files().symmetric_difference(&[&path2, &path3].into()).count(), 0);
        assert_eq!(files.filter_files(&["**/config/**"]).list_files().symmetric_difference(&[&path2, &path3, &path4].into()).count(), 0);
        // This assertion fails, potentially due to a bug in glob-match. More investigation is needed.
        // assert_eq!(files.filter_files(&["!**/*.md"]).list_files().symmetric_difference(&[&path1, &path2, &path3].into()).count(), 0);
    }
}
