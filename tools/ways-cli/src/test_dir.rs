//! A test directory that removes itself, contents and all, when dropped, so
//! a test run leaves nothing behind in the temp dir. Bind the guard to a name
//! that lives as long as the test, and as long as anything that was handed
//! the path: a temporary drops at the end of its statement, and `let _ =`
//! drops on the spot.

use std::path::{Path, PathBuf};

pub(crate) struct TestDir {
    top: PathBuf,
    path: PathBuf,
}

impl TestDir {
    /// An empty directory at `top`, replacing whatever a reused pid left.
    pub(crate) fn at(top: PathBuf) -> TestDir {
        let _ = std::fs::remove_dir_all(&top);
        std::fs::create_dir_all(&top).unwrap();
        TestDir { path: top.clone(), top }
    }

    /// The same directory, pointing at its resolved path (a temp dir that is
    /// itself a symlink, as on macOS).
    pub(crate) fn resolved(mut self) -> TestDir {
        if let Ok(p) = std::fs::canonicalize(&self.path) {
            self.path = p;
        }
        self
    }

    /// The same directory, pointing at `rel` inside it: the guard still
    /// removes the whole of it.
    pub(crate) fn under(mut self, rel: &str) -> TestDir {
        self.path = self.path.join(rel);
        self
    }
}

impl std::ops::Deref for TestDir {
    type Target = PathBuf;
    fn deref(&self) -> &PathBuf {
        &self.path
    }
}

impl AsRef<Path> for TestDir {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.top);
    }
}
