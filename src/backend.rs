use std::{
    future::Future,
    io::{self, Read},
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::SystemTime,
};

use anyhow::{Context, Result};
use cap_std::{ambient_authority, fs::Dir};
use tokio::io::{AsyncRead, AsyncSeek, ReadBuf};
use tower_http::services::fs::{Backend, File, Metadata};

#[derive(Clone)]
pub struct CapabilityBackend {
    root: Arc<Dir>,
}

impl CapabilityBackend {
    pub fn new(root: &Path) -> Result<Self> {
        let root = Dir::open_ambient_dir(root, ambient_authority())
            .with_context(|| format!("cannot open served directory {}", root.display()))?;
        Ok(Self {
            root: Arc::new(root),
        })
    }

    pub async fn read(&self, path: PathBuf) -> io::Result<Vec<u8>> {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || {
            let mut file = root.open(path)?;
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            Ok(bytes)
        })
        .await
        .map_err(io::Error::other)?
    }

    pub async fn read_bounded(&self, path: PathBuf, max_bytes: u64) -> io::Result<Option<Vec<u8>>> {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || {
            if root.metadata(&path)?.len() > max_bytes {
                return Ok(None);
            }
            let file = root.open(path)?;
            let mut bytes = Vec::new();
            file.take(max_bytes.saturating_add(1))
                .read_to_end(&mut bytes)?;
            if bytes.len() as u64 > max_bytes {
                Ok(None)
            } else {
                Ok(Some(bytes))
            }
        })
        .await
        .map_err(io::Error::other)?
    }

    pub async fn entries(&self, path: PathBuf) -> io::Result<Vec<(String, bool)>> {
        let root = self.root.clone();
        tokio::task::spawn_blocking(move || {
            let mut items = Vec::new();
            let path = if path.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                path
            };
            for entry in root.read_dir(path)? {
                let entry = entry?;
                let Ok(name) = entry.file_name().into_string() else {
                    continue;
                };
                items.push((name, entry.file_type()?.is_dir()));
            }
            Ok(items)
        })
        .await
        .map_err(io::Error::other)?
    }
}

impl Backend for CapabilityBackend {
    type File = CapabilityFile;
    type Metadata = FileMetadata;
    type OpenFuture = Pin<Box<dyn Future<Output = io::Result<Self::File>> + Send>>;
    type MetadataFuture = Pin<Box<dyn Future<Output = io::Result<Self::Metadata>> + Send>>;

    fn open(&self, path: PathBuf) -> Self::OpenFuture {
        let root = self.root.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || root.open(path).map(|file| file.into_std()))
                .await
                .map_err(io::Error::other)?
                .map(|file| CapabilityFile(tokio::fs::File::from_std(file)))
        })
    }

    fn metadata(&self, path: PathBuf) -> Self::MetadataFuture {
        let root = self.root.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || root.metadata(path).map(FileMetadata::from_cap))
                .await
                .map_err(io::Error::other)?
        })
    }
}

pub struct CapabilityFile(tokio::fs::File);

impl AsyncRead for CapabilityFile {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> std::task::Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_read(cx, buffer)
    }
}

impl AsyncSeek for CapabilityFile {
    fn start_seek(mut self: Pin<&mut Self>, position: io::SeekFrom) -> io::Result<()> {
        Pin::new(&mut self.0).start_seek(position)
    }

    fn poll_complete(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<io::Result<u64>> {
        Pin::new(&mut self.0).poll_complete(cx)
    }
}

impl File for CapabilityFile {
    type Metadata = FileMetadata;
    type MetadataFuture<'a> = Pin<Box<dyn Future<Output = io::Result<FileMetadata>> + Send + 'a>>;

    fn metadata(&self) -> Self::MetadataFuture<'_> {
        Box::pin(async move { self.0.metadata().await.map(FileMetadata::from_std) })
    }
}

pub struct FileMetadata {
    is_dir: bool,
    len: u64,
    modified: io::Result<SystemTime>,
}

impl FileMetadata {
    fn from_cap(metadata: cap_std::fs::Metadata) -> Self {
        Self {
            is_dir: metadata.is_dir(),
            len: metadata.len(),
            modified: metadata.modified().map(|time| time.into_std()),
        }
    }

    fn from_std(metadata: std::fs::Metadata) -> Self {
        Self {
            is_dir: metadata.is_dir(),
            len: metadata.len(),
            modified: metadata.modified(),
        }
    }
}

impl Metadata for FileMetadata {
    fn is_dir(&self) -> bool {
        self.is_dir
    }

    fn modified(&self) -> io::Result<SystemTime> {
        self.modified
            .as_ref()
            .copied()
            .map_err(|error| io::Error::new(error.kind(), error.to_string()))
    }

    fn len(&self) -> u64 {
        self.len
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[tokio::test]
    async fn cannot_open_a_symlink_outside_the_root() {
        let base = std::env::temp_dir().join(format!("srv-capability-{}", std::process::id()));
        let root = base.join("public");
        let outside = base.join("private.txt");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(&outside, "private").await.unwrap();
        symlink(&outside, root.join("leak")).unwrap();

        let backend = CapabilityBackend::new(&root).unwrap();
        assert!(
            Backend::open(&backend, PathBuf::from("leak"))
                .await
                .is_err()
        );
        tokio::fs::remove_dir_all(base).await.unwrap();
    }

    #[tokio::test]
    async fn bounded_reads_reject_files_over_the_limit() {
        let base = std::env::temp_dir().join(format!(
            "srv-bounded-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        tokio::fs::create_dir_all(&base).await.unwrap();
        tokio::fs::write(base.join("page.html"), "0123456789")
            .await
            .unwrap();

        let backend = CapabilityBackend::new(&base).unwrap();
        assert_eq!(
            backend
                .read_bounded(PathBuf::from("page.html"), 10)
                .await
                .unwrap(),
            Some(b"0123456789".to_vec())
        );
        assert_eq!(
            backend
                .read_bounded(PathBuf::from("page.html"), 9)
                .await
                .unwrap(),
            None
        );
        tokio::fs::remove_dir_all(base).await.unwrap();
    }
}
