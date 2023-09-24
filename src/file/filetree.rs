use super::filepath::FilePath;
use super::filestore::FileStore;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::u128;
use thiserror::Error;

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
        FileTree {
            contents: HashMap::new(),
            store,
        }
    }

    pub(crate) fn add_file(&mut self, path: FilePath, file: Vec<u8>) {
        let hash = self.store.write_file(file);
        self.contents.insert(path, hash);
    }

    pub(crate) fn get_file(&self, path: &FilePath) -> Option<Arc<Vec<u8>>> {
        if let Some(hash) = self.contents.get(path) {
            if let Some(file) = self.store.get_file(*hash) {
                return Some(file);
            }
        }
        None
    }

    /// Delete a filepath from the filetree.
    ///
    /// Idempotent.
    pub(crate) fn delete_file(&mut self, path: &FilePath) {
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
        self.contents.keys().collect()
    }

    /// Splits files into two cloned [`FileTree`] objects based on whether they match the provided
    /// filters. The first returned value contains the files that match the filters.
    pub(crate) fn filter_files<T: AsRef<str>>(&self, filters: &[T]) -> (FileTree, FileTree) {
        let (matched, inverse) = self
            .contents
            .iter()
            .map(|entry| (entry.0.clone(), *entry.1))
            .partition::<HashMap<FilePath, u128>, _>(|entry| entry.0.glob_match(filters));
        (
            FileTree {
                contents: matched,
                store: self.store.clone(),
            },
            FileTree {
                contents: inverse,
                store: self.store.clone(),
            },
        )
    }

    /// Add all files from another [`FileTree`] to this one, consuming the other in the process.
    /// If both [`FileTree`] objects share the same [`FileStore`], the hash references are simply copied
    /// over. Otherwise, the file data are copied them into this [`FileTree`]'s underlying [`FileStore`].
    /// Files in the consumed [`FileTree`] will overwrite files in this one with identical
    /// [`FilePath`]s.
    pub(crate) fn add_all(&mut self, other: FileTree) {
        match self.store == other.store {
            true => {
                self.contents.extend(other.contents.into_iter());
            },
            false => {
                other
                    .contents
                    .into_iter()
                    .filter_map(|(k, v)| other.store.get_file(v).map(|v| (k, v))) // Ignore None
                    .for_each(|(k, v)| {
                        self.add_file(k, v.as_ref().clone());
                    });
            },
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
        files.add_file(path.clone(), contents.into());

        assert_eq!(String::from_utf8(files.get_file(&path).unwrap().to_vec()).unwrap(), contents.to_string())
    }

    #[test]
    fn delete_file() {
        let mut files = get_filetree();
        let path = FilePath::from_str("directory/file.txt").unwrap();
        files.add_file(path.clone(), "Hello World!".into());

        files.delete_file(&path);
        assert_eq!(files.get_file(&path), None);
    }

    #[test]
    fn copy_file() {
        let mut files = get_filetree();
        let path = FilePath::from_str("directory/file.txt").unwrap();
        let path2 = FilePath::from_str("directory_two/file_two.txt").unwrap();
        files.add_file(path.clone(), "Hello World!".into());

        files.copy_file(&path, &path2).unwrap();
        assert_eq!(files.get_file(&path).unwrap(), files.get_file(&path2).unwrap());
    }

    #[test]
    fn copy_file_error_on_missing_source() {
        let mut files = get_filetree();
        let result = files.copy_file(
            &FilePath::from_str("path/does/not/exist.txt").unwrap(),
            &FilePath::from_str("destination/path.txt").unwrap(),
        );

        assert_eq!(
            result.unwrap_err(),
            FileTreeError::FileNotFoundError("path/does/not/exist.txt".to_string())
        );
    }

    #[test]
    fn move_file() {
        let mut files = get_filetree();
        let path = FilePath::from_str("directory/file.txt").unwrap();
        let path2 = FilePath::from_str("directory_two/file_two.txt").unwrap();
        let contents = "Hello World!";
        files.add_file(path.clone(), contents.into());
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

        files.add_file(path1.clone(), "Test".into());
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
        files.add_file(path1.clone(), "Hello".into());
        files.add_file(path2.clone(), "World".into());
        files.add_file(path3.clone(), "Foo".into());
        files.add_file(path4.clone(), "Bar".into());

        // Filter by root path
        assert_eq!(
            files
                .filter_files(&["overrides/**/*"])
                .0
                .list_files()
                .symmetric_difference(&[&path1, &path2].into())
                .count(),
            0
        );

        // Inverse
        assert_eq!(
            files
                .filter_files(&["overrides/**/*"])
                .1
                .list_files()
                .symmetric_difference(&[&path3, &path4].into())
                .count(),
            0
        );

        // Filter by file extension
        assert_eq!(
            files
                .filter_files(&["**/*.cfg"])
                .0
                .list_files()
                .symmetric_difference(&[&path2, &path3].into())
                .count(),
            0
        );

        // Filter by multiple file extensions
        assert_eq!(
            files
                .filter_files(&["**/*.jar", "**/*.cfg"])
                .0
                .list_files()
                .symmetric_difference(&[&path1, &path2, &path3].into())
                .count(),
            0
        );

        // Filter by root path pattern and file extension
        assert_eq!(
            files
                .filter_files(&["*overrides/**/*.cfg"])
                .0
                .list_files()
                .symmetric_difference(&[&path2, &path3].into())
                .count(),
            0
        );

        // Filter by subdirectory
        assert_eq!(
            files
                .filter_files(&["**/config/**"])
                .0
                .list_files()
                .symmetric_difference(&[&path2, &path3, &path4].into())
                .count(),
            0
        );

        // Exclude specific file
        assert_eq!(
            files
                .filter_files(&["other/config/readme.md"])
                .1
                .list_files()
                .symmetric_difference(&[&path1, &path2, &path3].into())
                .count(),
            0
        );
    }

    #[test]
    fn merge_distinct_contents_shared_store() {
        let store = FileStore::new();
        let mut tree1 = FileTree::new(store.clone());
        let mut tree2 = FileTree::new(store);
        tree1.add_file(FilePath::from_str("path1").unwrap(), "contents".into());
        tree2.add_file(FilePath::from_str("path2").unwrap(), "other".into());
        tree1.add_all(tree2);
        assert_eq!(tree1.list_files().len(), 2);
        assert_eq!(
            std::str::from_utf8(&tree1.get_file(&FilePath::from_str("path1").unwrap()).unwrap()).unwrap(),
            "contents"
        );
        assert_eq!(
            std::str::from_utf8(&tree1.get_file(&FilePath::from_str("path2").unwrap()).unwrap()).unwrap(),
            "other"
        );
    }

    #[test]
    fn merge_overlapping_contents() {
        let store = FileStore::new();
        let mut tree1 = FileTree::new(store.clone());
        let mut tree2 = FileTree::new(store);
        tree1.add_file(FilePath::from_str("from/tree1.txt").unwrap(), "a".into());
        tree2.add_file(FilePath::from_str("from/tree2.txt").unwrap(), "b".into());
        tree1.add_file(FilePath::from_str("shared").unwrap(), "c".into());
        tree2.add_file(FilePath::from_str("shared").unwrap(), "d".into());
        tree1.add_all(tree2);
        assert_eq!(tree1.list_files().len(), 3);
        assert_eq!(
            std::str::from_utf8(&tree1.get_file(&FilePath::from_str("from/tree1.txt").unwrap()).unwrap()).unwrap(),
            "a"
        );
        assert_eq!(
            std::str::from_utf8(&tree1.get_file(&FilePath::from_str("from/tree2.txt").unwrap()).unwrap()).unwrap(),
            "b"
        );

        assert_eq!(
            std::str::from_utf8(&tree1.get_file(&FilePath::from_str("shared").unwrap()).unwrap()).unwrap(),
            "d"
        );
    }

    #[test]
    fn merge_distinct_store() {
        let store1 = FileStore::new();
        let store2 = FileStore::new();
        let mut tree1 = FileTree::new(store1);
        let mut tree2 = FileTree::new(store2);
        tree1.add_file(FilePath::from_str("path1").unwrap(), "contents".into());
        tree2.add_file(FilePath::from_str("path2").unwrap(), "other".into());
        tree1.add_all(tree2);
        assert_eq!(tree1.list_files().len(), 2);
        assert_eq!(
            std::str::from_utf8(&tree1.get_file(&FilePath::from_str("path1").unwrap()).unwrap()).unwrap(),
            "contents"
        );
        assert_eq!(
            std::str::from_utf8(&tree1.get_file(&FilePath::from_str("path2").unwrap()).unwrap()).unwrap(),
            "other"
        );
    }
}
