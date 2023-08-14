use std::collections::HashMap;

use anyhow::{Result, bail};

use super::filestore::FileStore;
use super::filepath::FilePath;

#[derive(Clone)]
pub(crate) struct Filetree {
    contents: HashMap<String, FileType>, // Do we need to support non-UTF8 filenames (OsString)?
    store: FileStore,
}

#[derive(Clone)]
pub(crate) struct Inode {
    data: Box<FileType>,
    // Add permissions or other attributes?
}

#[derive(Clone)]
pub(crate) enum FileType {
    File(u128),
    Directory(Filetree),
}


impl Filetree {
    pub(crate) fn new(store: FileStore) -> Filetree {
        Filetree {
            contents: HashMap::new(),
            store,
        }
    }

    pub(crate) fn add_file(&mut self, path: &FilePath, file: Vec<u8>) -> Result<()> {
        self.add_file_recursive(path, file, 0)
    }

    fn add_file_recursive(&mut self, path: &FilePath, file: Vec<u8>, level: usize) -> Result<()> {
        // Get directory name at current recursion level
        match path.get_dir_at(level) {
            // We have not yet reached where the file should go
            Some(dir_name) => {
                // Next file/directory exists already
                if let Some(inode) = self.contents.remove(dir_name) {
                    if let FileType::Directory(mut subtree) = inode {
                        subtree.add_file_recursive(path, file, level + 1)?;
                        self.contents.insert(dir_name.to_owned(), FileType::Directory(subtree));
                    } else {
                        self.contents.insert(dir_name.to_owned(), inode);
                        bail!("Directory path overlaps with a file!");
                    }
                // We need to create a new directory
                } else {
                    let mut subtree = Filetree::new(self.store.clone());
                    subtree.add_file_recursive(path, file, level + 1)?;
                    self.contents.insert(dir_name.to_owned(), FileType::Directory(subtree));
                }
            },
            // We have reached the level where the file should go
            None => {
                let hash = self.store.write_file(file);
                self.contents.insert(path.get_filename().to_owned(), FileType::File(hash));
            },
        }
        Ok(())
    }
}
