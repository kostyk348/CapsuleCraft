//! File-dialog shim: real native dialogs on the desktop, `None` on Android
//! until the SAF picker lands (docs/android.md, roadmap #1). The API mirrors
//! the small subset of `crate::util::dialog::FileDialog` the app uses, so every call site
//! just swaps the constructor.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

pub struct FileDialog {
    #[cfg(not(target_os = "android"))]
    inner: rfd::FileDialog,
}

impl Default for FileDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl FileDialog {
    pub fn new() -> Self {
        Self {
            #[cfg(not(target_os = "android"))]
            inner: rfd::FileDialog::new(),
        }
    }

    pub fn set_title(self, title: impl Into<String>) -> Self {
        #[cfg(not(target_os = "android"))]
        {
            return Self {
                inner: self.inner.set_title(title.into()),
            };
        }
        #[cfg(target_os = "android")]
        {
            let _ = title;
            self
        }
    }

    pub fn set_directory(self, dir: &Path) -> Self {
        #[cfg(not(target_os = "android"))]
        {
            return Self {
                inner: self.inner.set_directory(dir),
            };
        }
        #[cfg(target_os = "android")]
        {
            let _ = dir;
            self
        }
    }

    pub fn add_filter(self, name: impl Into<String>, extensions: &[&str]) -> Self {
        #[cfg(not(target_os = "android"))]
        {
            return Self {
                inner: self.inner.add_filter(name, extensions),
            };
        }
        #[cfg(target_os = "android")]
        {
            let _ = (name, extensions);
            self
        }
    }

    pub fn set_file_name(self, file_name: impl Into<String>) -> Self {
        #[cfg(not(target_os = "android"))]
        {
            return Self {
                inner: self.inner.set_file_name(file_name),
            };
        }
        #[cfg(target_os = "android")]
        {
            let _ = file_name;
            self
        }
    }

    pub fn pick_file(self) -> Option<PathBuf> {
        #[cfg(not(target_os = "android"))]
        {
            self.inner.pick_file()
        }
        #[cfg(target_os = "android")]
        {
            None
        }
    }

    pub fn pick_files(self) -> Option<Vec<PathBuf>> {
        #[cfg(not(target_os = "android"))]
        {
            self.inner.pick_files()
        }
        #[cfg(target_os = "android")]
        {
            None
        }
    }

    pub fn pick_folder(self) -> Option<PathBuf> {
        #[cfg(not(target_os = "android"))]
        {
            self.inner.pick_folder()
        }
        #[cfg(target_os = "android")]
        {
            None
        }
    }

    pub fn save_file(self) -> Option<PathBuf> {
        #[cfg(not(target_os = "android"))]
        {
            self.inner.save_file()
        }
        #[cfg(target_os = "android")]
        {
            None
        }
    }
}
