//! Keeps Omalogi processes from using the device during another's memory write.
//!
//! Replies are already kept apart by HID++ software id, but a flash write is a
//! sequence of requests; the CLI holds this lock for the whole backup, write,
//! verify and rollback, and the daemon only polls when it can take the lock at once.

use std::{
    fs::{self, File, OpenOptions},
    io,
    os::fd::AsRawFd,
    path::{Path, PathBuf},
};

/// An exclusive `flock` on the lock file, released when dropped.
#[derive(Debug)]
pub struct DeviceLock {
    _file: File,
}

impl DeviceLock {
    /// `$XDG_RUNTIME_DIR/omalogi/device.lock`, when a runtime directory exists.
    #[must_use]
    pub fn default_path() -> Option<PathBuf> {
        std::env::var_os("XDG_RUNTIME_DIR")
            .filter(|dir| !dir.is_empty())
            .map(|dir| PathBuf::from(dir).join("omalogi/device.lock"))
    }

    /// Waits until no other Omalogi process holds the lock.
    pub fn acquire(path: &Path) -> io::Result<Self> {
        let file = open(path)?;
        flock(&file, libc::LOCK_EX)?;
        Ok(Self { _file: file })
    }

    /// The lock if it is free right now, or `None` while another process holds it.
    pub fn try_acquire(path: &Path) -> io::Result<Option<Self>> {
        let file = open(path)?;
        match flock(&file, libc::LOCK_EX | libc::LOCK_NB) {
            Ok(()) => Ok(Some(Self { _file: file })),
            Err(error) if error.raw_os_error() == Some(libc::EWOULDBLOCK) => Ok(None),
            Err(error) => Err(error),
        }
    }
}

fn open(path: &Path) -> io::Result<File> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

fn flock(file: &File, operation: libc::c_int) -> io::Result<()> {
    loop {
        // SAFETY: `file` owns a valid open descriptor for the duration of the call.
        if unsafe { libc::flock(file.as_raw_fd(), operation) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(libc::EINTR) {
            return Err(error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock_path(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("omalogi-lock-{name}-{}", std::process::id()))
            .join("device.lock")
    }

    #[test]
    fn a_held_lock_is_not_available_until_dropped() {
        let path = lock_path("held");
        let held = DeviceLock::acquire(&path).expect("first lock");
        assert!(
            DeviceLock::try_acquire(&path).expect("try lock").is_none(),
            "second open file must not get the lock"
        );
        drop(held);
        assert!(DeviceLock::try_acquire(&path).expect("try lock").is_some());
        fs::remove_dir_all(path.parent().expect("parent")).expect("clean up");
    }

    #[test]
    fn creates_the_runtime_directory() {
        let path = lock_path("create");
        let _lock = DeviceLock::try_acquire(&path)
            .expect("try lock")
            .expect("free");
        assert!(path.exists());
        fs::remove_dir_all(path.parent().expect("parent")).expect("clean up");
    }
}
