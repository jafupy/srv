use std::{
    future::Future,
    io::{self, Read},
    path::{Component, Path, PathBuf},
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
    show_hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryEntry {
    pub name: String,
    pub is_dir: bool,
    pub len: u64,
}

impl CapabilityBackend {
    pub fn new(root: &Path, show_hidden: bool) -> Result<Self> {
        let root = Dir::open_ambient_dir(root, ambient_authority())
            .with_context(|| format!("cannot open served directory {}", root.display()))?;
        Ok(Self {
            root: Arc::new(root),
            show_hidden,
        })
    }

    pub async fn read(&self, path: PathBuf) -> io::Result<Vec<u8>> {
        self.ensure_visible(&path)?;
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

    pub async fn entries(&self, path: PathBuf) -> io::Result<Vec<DirectoryEntry>> {
        let path = if path.as_os_str().is_empty() {
            PathBuf::from(".")
        } else {
            path
        };
        self.ensure_visible(&path)?;
        let root = self.root.clone();
        let show_hidden = self.show_hidden;
        tokio::task::spawn_blocking(move || {
            let mut items = Vec::new();
            for entry in root.read_dir(path)? {
                let entry = entry?;
                let Ok(name) = entry.file_name().into_string() else {
                    continue;
                };
                if !show_hidden && name.starts_with('.') {
                    continue;
                }
                let metadata = entry.metadata()?;
                items.push(DirectoryEntry {
                    name,
                    is_dir: metadata.is_dir(),
                    len: metadata.len(),
                });
            }
            Ok(items)
        })
        .await
        .map_err(io::Error::other)?
    }

    fn ensure_visible(&self, path: &Path) -> io::Result<()> {
        if self.show_hidden || !has_hidden_component(path) {
            return Ok(());
        }
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "hidden paths are disabled",
        ))
    }
}

fn has_hidden_component(path: &Path) -> bool {
    path.components().any(|component| {
        let Component::Normal(name) = component else {
            return false;
        };
        name.to_string_lossy().starts_with('.')
    })
}

impl Backend for CapabilityBackend {
    type File = CapabilityFile;
    type Metadata = FileMetadata;
    type OpenFuture = Pin<Box<dyn Future<Output = io::Result<Self::File>> + Send>>;
    type MetadataFuture = Pin<Box<dyn Future<Output = io::Result<Self::Metadata>> + Send>>;

    fn open(&self, path: PathBuf) -> Self::OpenFuture {
        if let Err(error) = self.ensure_visible(&path) {
            return Box::pin(async move { Err(error) });
        }
        let root = self.root.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || root.open(path).map(|file| file.into_std()))
                .await
                .map_err(io::Error::other)?
                .map(|file| CapabilityFile(tokio::fs::File::from_std(file)))
        })
    }

    fn metadata(&self, path: PathBuf) -> Self::MetadataFuture {
        if let Err(error) = self.ensure_visible(&path) {
            return Box::pin(async move { Err(error) });
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_hidden_components() {
        assert!(has_hidden_component(Path::new(".env")));
        assert!(has_hidden_component(Path::new("assets/.private/key")));
        assert!(!has_hidden_component(Path::new("assets/app.js")));
    }

    #[tokio::test]
    async fn hides_dotfiles_from_directory_listings() {
        let base = std::env::temp_dir().join(format!("srv-hidden-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&base).await;
        tokio::fs::create_dir_all(&base).await.unwrap();
        tokio::fs::write(base.join("visible.txt"), "ok")
            .await
            .unwrap();
        tokio::fs::write(base.join(".secret"), "no").await.unwrap();

        let backend = CapabilityBackend::new(&base, false).unwrap();
        let entries = backend.entries(PathBuf::new()).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "visible.txt");

        tokio::fs::remove_dir_all(base).await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cannot_open_a_symlink_outside_the_root() {
        use std::os::unix::fs::symlink;

        let base = std::env::temp_dir().join(format!("srv-capability-{}", std::process::id()));
        let _ = tokio::fs::remove_dir_all(&base).await;
        let root = base.join("public");
        let outside = base.join("private.txt");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(&outside, "private").await.unwrap();
        symlink(&outside, root.join("leak")).unwrap();

        let backend = CapabilityBackend::new(&root, false).unwrap();
        assert!(
            Backend::open(&backend, PathBuf::from("leak"))
                .await
                .is_err()
        );
        tokio::fs::remove_dir_all(base).await.unwrap();
    }
}
