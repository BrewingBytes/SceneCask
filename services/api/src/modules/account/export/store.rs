//! Private export artifact storage: one JSON file per finished claim in a directory that only the
//! API process can read. Files are never served by path; `result_path` holds a bare file name the
//! store resolves inside its own root, and nothing outside the store learns where that root is.

use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};

use uuid::Uuid;

#[derive(Clone)]
pub struct ExportStore {
    root: Arc<PathBuf>,
}

impl ExportStore {
    /// Opens (creating if needed) the artifact directory, restricted to the owner on Unix.
    pub fn open(root: impl Into<PathBuf>) -> io::Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self {
            root: Arc::new(root),
        })
    }

    /// The file name for `job`'s `attempts`-th claim. A claim that lost its lease never shares a
    /// name with the claim that took over, so it can remove its own file without touching theirs.
    pub fn file_name(job: Uuid, attempts: i32) -> String {
        format!("{}-{attempts}.json", job.simple())
    }

    /// Writes `bytes` under `name` atomically: a temporary file is synced and renamed, so a crash
    /// leaves either no artifact or a complete one, never a partial file under the final name.
    pub async fn write(&self, name: String, bytes: Vec<u8>) -> io::Result<()> {
        let path = self.resolve(&name)?;
        blocking(move || {
            let temporary = path.with_extension("tmp");
            let mut options = fs::OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let result = (|| {
                use std::io::Write;
                let mut file = options.open(&temporary)?;
                file.write_all(&bytes)?;
                file.sync_all()?;
                fs::rename(&temporary, &path)
            })();
            if result.is_err() {
                let _ = fs::remove_file(&temporary);
            }
            result
        })
        .await
    }

    pub async fn read(&self, name: &str) -> io::Result<Vec<u8>> {
        let path = self.resolve(name)?;
        blocking(move || fs::read(path)).await
    }

    /// Removes `name`; an already missing file counts as removed.
    pub async fn remove(&self, name: &str) -> io::Result<()> {
        let path = self.resolve(name)?;
        blocking(move || match fs::remove_file(path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        })
        .await
    }

    /// Removes files older than `grace` that are not in `keep`: artifacts and temporary files a
    /// crashed or superseded claim left behind. The grace period spares a claim that has written
    /// its file but not yet recorded it. Returns how many files were removed.
    pub async fn sweep(&self, keep: HashSet<String>, grace: Duration) -> io::Result<usize> {
        let root = Arc::clone(&self.root);
        blocking(move || {
            let cutoff = SystemTime::now()
                .checked_sub(grace)
                .unwrap_or(SystemTime::UNIX_EPOCH);
            let mut removed = 0;
            for entry in fs::read_dir(&*root)? {
                let entry = entry?;
                let Ok(name) = entry.file_name().into_string() else {
                    continue;
                };
                let metadata = entry.metadata()?;
                if !metadata.is_file() || keep.contains(&name) || metadata.modified()? > cutoff {
                    continue;
                }
                match fs::remove_file(entry.path()) {
                    Ok(()) => removed += 1,
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
            Ok(removed)
        })
        .await
    }

    /// `name` inside the root. Only a single plain path component is accepted, so a stored value
    /// can never point outside the store.
    fn resolve(&self, name: &str) -> io::Result<PathBuf> {
        let plain = !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
            && Path::new(name).file_name().is_some_and(|file| file == name);
        if plain {
            Ok(self.root.join(name))
        } else {
            Err(io::ErrorKind::InvalidInput.into())
        }
    }
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or_else(|_| Err(io::Error::other("storage task failed")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (ExportStore, PathBuf) {
        let root = std::env::temp_dir().join(format!("scenecask-store-{}", Uuid::new_v4()));
        (ExportStore::open(&root).unwrap(), root)
    }

    #[tokio::test]
    async fn writes_reads_and_removes_without_leaving_temporary_files() {
        let (store, root) = store();
        let name = ExportStore::file_name(Uuid::from_u128(7), 2);
        assert_eq!(name, format!("{}-2.json", Uuid::from_u128(7).simple()));
        store.write(name.clone(), b"{}".to_vec()).await.unwrap();
        assert_eq!(store.read(&name).await.unwrap(), b"{}");
        let files: Vec<_> = fs::read_dir(&root).unwrap().collect();
        assert_eq!(files.len(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode(&root), 0o700);
            assert_eq!(mode(&root.join(&name)), 0o600);
        }
        store.remove(&name).await.unwrap();
        store.remove(&name).await.unwrap();
        assert_eq!(
            store.read(&name).await.unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn rejects_names_that_leave_the_root() {
        let (store, root) = store();
        for name in [
            "",
            "..",
            "../x.json",
            "a/b.json",
            "/etc/passwd",
            "x\\y.json",
        ] {
            assert_eq!(
                store.read(name).await.unwrap_err().kind(),
                io::ErrorKind::InvalidInput,
                "{name}"
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn sweep_keeps_listed_and_recent_files() {
        let (store, root) = store();
        for name in ["kept.json", "orphan.json", "orphan.tmp"] {
            fs::write(root.join(name), b"{}").unwrap();
        }
        let keep = HashSet::from(["kept.json".to_owned()]);
        assert_eq!(
            store
                .sweep(keep.clone(), Duration::from_secs(3600))
                .await
                .unwrap(),
            0
        );
        assert_eq!(store.sweep(keep, Duration::ZERO).await.unwrap(), 2);
        let left: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, ["kept.json"]);
        fs::remove_dir_all(root).unwrap();
    }
}
