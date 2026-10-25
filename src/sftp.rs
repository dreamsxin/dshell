use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result};
use russh_sftp::{
    client::SftpSession,
    protocol::{FileAttributes, OpenFlags},
};
use tokio::{
    io::{
        AsyncRead, AsyncReadExt as _, AsyncSeekExt as _, AsyncWrite, AsyncWriteExt as _, SeekFrom,
    },
    sync::mpsc,
};

const TRANSFER_CHUNK_BYTES: usize = 32 * 1024;
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);
const MAX_RECURSIVE_ENTRIES: usize = 10_000;
const MAX_RECURSIVE_DEPTH: usize = 32;

fn resume_offset(existing: u64, total: u64) -> u64 {
    if existing <= total { existing } else { 0 }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TransferKind {
    Upload,
    Download,
}

impl TransferKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Upload => "Upload",
            Self::Download => "Download",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub permissions: Option<u32>,
    pub atime: Option<u32>,
    pub mtime: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoteFilesystemInfo {
    pub path: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub available_bytes: u64,
    pub block_size: u64,
    pub name_max: u64,
}

#[derive(Clone, Debug)]
pub enum SftpCommand {
    List(String),
    Upload {
        local: PathBuf,
        remote: String,
        overwrite: bool,
        resume: bool,
    },
    Download {
        remote: String,
        local: PathBuf,
        overwrite: bool,
        resume: bool,
    },
    UploadDirectory {
        local: PathBuf,
        remote: String,
        overwrite: bool,
        resume: bool,
    },
    DownloadDirectory {
        remote: String,
        local: PathBuf,
        overwrite: bool,
        resume: bool,
    },
    Rename {
        from: String,
        to: String,
    },
    CreateDirectory(String),
    RemoveFile(String),
    RemoveDirectory(String),
    SetMetadata {
        path: String,
        permissions: Option<u32>,
        atime: Option<u32>,
        mtime: Option<u32>,
    },
    FilesystemInfo(String),
}

#[derive(Debug)]
pub enum SftpEvent {
    Ready {
        path: String,
    },
    Entries {
        path: String,
        entries: Vec<RemoteEntry>,
    },
    FilesystemInfo(RemoteFilesystemInfo),
    FilesystemUnavailable {
        path: String,
    },
    Error(String),
    NeedsOverwrite {
        kind: TransferKind,
        local: PathBuf,
        remote: String,
        is_directory: bool,
    },
    TransferStarted {
        kind: TransferKind,
        name: String,
        total: u64,
    },
    TransferProgress {
        kind: TransferKind,
        name: String,
        transferred: u64,
        total: u64,
    },
    TransferFinished {
        kind: TransferKind,
        name: String,
    },
    TransferFailed {
        kind: TransferKind,
        name: String,
        error: String,
    },
    TransferCancelled {
        kind: TransferKind,
        name: String,
    },
    OperationFinished {
        action: String,
    },
}

pub async fn run<S>(
    stream: S,
    mut commands: mpsc::Receiver<SftpCommand>,
    events: mpsc::Sender<SftpEvent>,
    cancellation: Arc<AtomicBool>,
) where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let session = match SftpSession::new(stream).await {
        Ok(session) => session,
        Err(error) => {
            let _ = events
                .send(SftpEvent::Error(format!(
                    "Could not start the SFTP subsystem: {error}"
                )))
                .await;
            return;
        }
    };
    let home = session
        .canonicalize(".")
        .await
        .unwrap_or_else(|_| ".".to_owned());
    if events
        .send(SftpEvent::Ready { path: home.clone() })
        .await
        .is_err()
    {
        return;
    }
    list(&session, &home, &events).await;

    while let Some(command) = commands.recv().await {
        match command {
            SftpCommand::List(path) => list(&session, &path, &events).await,
            SftpCommand::Upload {
                local,
                remote,
                overwrite,
                resume,
            } => {
                upload(
                    &session,
                    local,
                    remote,
                    overwrite,
                    resume,
                    &events,
                    &cancellation,
                )
                .await;
            }
            SftpCommand::Download {
                remote,
                local,
                overwrite,
                resume,
            } => {
                download(
                    &session,
                    remote,
                    local,
                    overwrite,
                    resume,
                    &events,
                    &cancellation,
                )
                .await;
            }
            SftpCommand::UploadDirectory {
                local,
                remote,
                overwrite,
                resume,
            } => {
                upload_directory(
                    &session,
                    local,
                    remote,
                    overwrite,
                    resume,
                    &events,
                    &cancellation,
                )
                .await;
            }
            SftpCommand::DownloadDirectory {
                remote,
                local,
                overwrite,
                resume,
            } => {
                download_directory(
                    &session,
                    remote,
                    local,
                    overwrite,
                    resume,
                    &events,
                    &cancellation,
                )
                .await;
            }
            SftpCommand::Rename { from, to } => {
                match session.rename(from.clone(), to.clone()).await {
                    Ok(()) => {
                        let _ = events
                            .send(SftpEvent::OperationFinished {
                                action: format!("Renamed {from} to {to}"),
                            })
                            .await;
                    }
                    Err(error) => {
                        let _ = events
                            .send(SftpEvent::Error(format!(
                                "Could not rename {from} to {to}: {error}"
                            )))
                            .await;
                    }
                }
            }
            SftpCommand::CreateDirectory(path) => match session.create_dir(path.clone()).await {
                Ok(()) => {
                    let _ = events
                        .send(SftpEvent::OperationFinished {
                            action: format!("Created directory {path}"),
                        })
                        .await;
                }
                Err(error) => {
                    let _ = events
                        .send(SftpEvent::Error(format!(
                            "Could not create {path}: {error}"
                        )))
                        .await;
                }
            },
            SftpCommand::RemoveFile(path) => match session.remove_file(path.clone()).await {
                Ok(()) => {
                    let _ = events
                        .send(SftpEvent::OperationFinished {
                            action: format!("Deleted file {path}"),
                        })
                        .await;
                }
                Err(error) => {
                    let _ = events
                        .send(SftpEvent::Error(format!(
                            "Could not delete {path}: {error}"
                        )))
                        .await;
                }
            },
            SftpCommand::RemoveDirectory(path) => match session.remove_dir(path.clone()).await {
                Ok(()) => {
                    let _ = events
                        .send(SftpEvent::OperationFinished {
                            action: format!("Removed directory {path}"),
                        })
                        .await;
                }
                Err(error) => {
                    let _ = events
                        .send(SftpEvent::Error(format!(
                            "Could not remove {path}: {error}"
                        )))
                        .await;
                }
            },
            SftpCommand::SetMetadata {
                path,
                permissions,
                atime,
                mtime,
            } => {
                let metadata = FileAttributes {
                    permissions,
                    atime,
                    mtime,
                    ..FileAttributes::default()
                };
                match session.set_metadata(path.clone(), metadata).await {
                    Ok(()) => {
                        let _ = events
                            .send(SftpEvent::OperationFinished {
                                action: format!("Updated metadata for {path}"),
                            })
                            .await;
                    }
                    Err(error) => {
                        let _ = events
                            .send(SftpEvent::Error(format!(
                                "Could not update metadata for {path}: {error}"
                            )))
                            .await;
                    }
                }
            }
            SftpCommand::FilesystemInfo(path) => match session.fs_info(path.clone()).await {
                Ok(Some(stats)) => {
                    let unit = stats.fragment_size.max(1);
                    let _ = events
                        .send(SftpEvent::FilesystemInfo(RemoteFilesystemInfo {
                            path,
                            total_bytes: stats.blocks.saturating_mul(unit),
                            free_bytes: stats.blocks_free.saturating_mul(unit),
                            available_bytes: stats.blocks_avail.saturating_mul(unit),
                            block_size: stats.block_size,
                            name_max: stats.name_max,
                        }))
                        .await;
                }
                Ok(None) => {
                    let _ = events.send(SftpEvent::FilesystemUnavailable { path }).await;
                }
                Err(error) => {
                    let _ = events
                        .send(SftpEvent::Error(format!(
                            "Could not read filesystem information for {path}: {error}"
                        )))
                        .await;
                }
            },
        }
    }
    let _ = session.close().await;
}

async fn list(session: &SftpSession, path: &str, events: &mpsc::Sender<SftpEvent>) {
    match read_entries(session, path).await {
        Ok(entries) => {
            let _ = events
                .send(SftpEvent::Entries {
                    path: path.to_owned(),
                    entries,
                })
                .await;
        }
        Err(error) => {
            let _ = events
                .send(SftpEvent::Error(format!(
                    "Could not list {path}: {error:#}"
                )))
                .await;
        }
    }
}

async fn read_entries(session: &SftpSession, path: &str) -> Result<Vec<RemoteEntry>> {
    let entries = session
        .read_dir(path.to_owned())
        .await
        .with_context(|| format!("SFTP read_dir failed for {path}"))?;
    let mut result = entries
        .filter(|entry| {
            let name = entry.file_name();
            name != "." && name != ".."
        })
        .map(|entry| {
            let metadata = entry.metadata();
            RemoteEntry {
                name: entry.file_name(),
                path: entry.path(),
                is_dir: entry.file_type().is_dir(),
                size: metadata.size.unwrap_or(0),
                permissions: metadata.permissions,
                atime: metadata.atime,
                mtime: metadata.mtime,
            }
        })
        .collect::<Vec<_>>();
    result.sort_by(|left, right| {
        right
            .is_dir
            .cmp(&left.is_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    Ok(result)
}

async fn upload(
    session: &SftpSession,
    local: PathBuf,
    remote: String,
    overwrite: bool,
    resume: bool,
    events: &mpsc::Sender<SftpEvent>,
    cancellation: &AtomicBool,
) {
    let kind = TransferKind::Upload;
    let name = local.file_name().map_or_else(
        || local.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let existed = session.try_exists(remote.clone()).await.unwrap_or(false);
    if existed && !overwrite && !resume {
        let _ = events
            .send(SftpEvent::NeedsOverwrite {
                kind,
                local,
                remote,
                is_directory: false,
            })
            .await;
        return;
    }

    let mut local_file = match tokio::fs::File::open(&local).await {
        Ok(file) => file,
        Err(error) => {
            fail(
                events,
                kind,
                &name,
                format!("Could not open {}: {error}", local.display()),
            )
            .await;
            return;
        }
    };
    let total = local_file
        .metadata()
        .await
        .map_or(0, |metadata| metadata.len());
    let existing = if resume && existed {
        session
            .metadata(remote.clone())
            .await
            .ok()
            .and_then(|metadata| metadata.size)
            .unwrap_or(0)
    } else {
        0
    };
    let mut offset = if resume {
        resume_offset(existing, total)
    } else {
        0
    };
    if offset == total && total > 0 {
        cancellation.store(false, Ordering::SeqCst);
        if events
            .send(SftpEvent::TransferStarted {
                kind,
                name: name.clone(),
                total,
            })
            .await
            .is_err()
        {
            return;
        }
        let _ = report_progress(events, kind, &name, total, total).await;
        let _ = events
            .send(SftpEvent::TransferFinished { kind, name })
            .await;
        return;
    }
    let mut remote_file = if offset > 0 {
        match session
            .open_with_flags(remote.clone(), OpenFlags::WRITE)
            .await
        {
            Ok(file) => file,
            Err(error) => {
                fail(
                    events,
                    kind,
                    &name,
                    format!("Could not reopen {remote} for resume: {error}"),
                )
                .await;
                return;
            }
        }
    } else {
        offset = 0;
        match session.create(remote.clone()).await {
            Ok(file) => file,
            Err(error) => {
                fail(
                    events,
                    kind,
                    &name,
                    format!("Could not create {remote}: {error}"),
                )
                .await;
                return;
            }
        }
    };
    if offset > 0 {
        if let Err(error) = remote_file.seek(SeekFrom::Start(offset)).await {
            fail(
                events,
                kind,
                &name,
                format!("Could not seek {remote} for resume: {error}"),
            )
            .await;
            return;
        }
        if let Err(error) = local_file.seek(SeekFrom::Start(offset)).await {
            fail(
                events,
                kind,
                &name,
                format!("Could not seek {} for resume: {error}", local.display()),
            )
            .await;
            return;
        }
    }

    cancellation.store(false, Ordering::SeqCst);
    if events
        .send(SftpEvent::TransferStarted {
            kind,
            name: name.clone(),
            total,
        })
        .await
        .is_err()
    {
        return;
    }
    if offset > 0 {
        let _ = report_progress(events, kind, &name, offset, total).await;
    }

    let mut buffer = vec![0u8; TRANSFER_CHUNK_BYTES];
    let mut transferred = offset;
    let mut last_report = Instant::now();
    loop {
        if cancellation.load(Ordering::SeqCst) {
            drop(remote_file);
            if offset == 0 {
                let _ = session.remove_file(remote).await;
            }
            let _ = events
                .send(SftpEvent::TransferCancelled { kind, name })
                .await;
            return;
        }
        let read = match local_file.read(&mut buffer).await {
            Ok(read) => read,
            Err(error) => {
                drop(remote_file);
                if offset == 0 {
                    let _ = session.remove_file(remote.clone()).await;
                }
                fail(
                    events,
                    kind,
                    &name,
                    format!("Could not read {}: {error}", local.display()),
                )
                .await;
                return;
            }
        };
        if read == 0 {
            break;
        }
        if let Err(error) = remote_file.write_all(&buffer[..read]).await {
            drop(remote_file);
            if offset == 0 {
                let _ = session.remove_file(remote.clone()).await;
            }
            fail(
                events,
                kind,
                &name,
                format!("Could not write {remote}: {error}"),
            )
            .await;
            return;
        }
        transferred += read as u64;
        if last_report.elapsed() >= PROGRESS_INTERVAL {
            last_report = Instant::now();
            if !report_progress(events, kind, &name, transferred, total).await {
                return;
            }
        }
    }
    if let Err(error) = remote_file.shutdown().await {
        drop(remote_file);
        if offset == 0 {
            let _ = session.remove_file(remote.clone()).await;
        }
        fail(
            events,
            kind,
            &name,
            format!("Could not finish {remote}: {error}"),
        )
        .await;
        return;
    }
    let _ = report_progress(events, kind, &name, transferred, total).await;
    let _ = events
        .send(SftpEvent::TransferFinished { kind, name })
        .await;
}

async fn download(
    session: &SftpSession,
    remote: String,
    local: PathBuf,
    overwrite: bool,
    resume: bool,
    events: &mpsc::Sender<SftpEvent>,
    cancellation: &AtomicBool,
) {
    let kind = TransferKind::Download;
    let name = local.file_name().map_or_else(
        || local.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let existed = tokio::fs::metadata(&local).await.is_ok();
    if existed && !overwrite && !resume {
        let _ = events
            .send(SftpEvent::NeedsOverwrite {
                kind,
                local,
                remote,
                is_directory: false,
            })
            .await;
        return;
    }

    let mut remote_file = match session.open(remote.clone()).await {
        Ok(file) => file,
        Err(error) => {
            fail(
                events,
                kind,
                &name,
                format!("Could not open {remote}: {error}"),
            )
            .await;
            return;
        }
    };
    let total = remote_file
        .metadata()
        .await
        .map_or(0, |metadata| metadata.size.unwrap_or(0));
    let existing = if resume && existed {
        tokio::fs::metadata(&local)
            .await
            .map_or(0, |metadata| metadata.len())
    } else {
        0
    };
    let mut offset = if resume {
        resume_offset(existing, total)
    } else {
        0
    };
    if offset == total && total > 0 {
        cancellation.store(false, Ordering::SeqCst);
        if events
            .send(SftpEvent::TransferStarted {
                kind,
                name: name.clone(),
                total,
            })
            .await
            .is_err()
        {
            return;
        }
        let _ = report_progress(events, kind, &name, total, total).await;
        let _ = events
            .send(SftpEvent::TransferFinished { kind, name })
            .await;
        return;
    }
    let mut local_file = if offset > 0 {
        match tokio::fs::OpenOptions::new()
            .append(true)
            .open(&local)
            .await
        {
            Ok(file) => file,
            Err(error) => {
                fail(
                    events,
                    kind,
                    &name,
                    format!("Could not reopen {} for resume: {error}", local.display()),
                )
                .await;
                return;
            }
        }
    } else {
        offset = 0;
        match tokio::fs::File::create(&local).await {
            Ok(file) => file,
            Err(error) => {
                fail(
                    events,
                    kind,
                    &name,
                    format!("Could not create {}: {error}", local.display()),
                )
                .await;
                return;
            }
        }
    };
    if offset > 0 {
        if let Err(error) = remote_file.seek(SeekFrom::Start(offset)).await {
            fail(
                events,
                kind,
                &name,
                format!("Could not seek {remote} for resume: {error}"),
            )
            .await;
            return;
        }
    }

    cancellation.store(false, Ordering::SeqCst);
    if events
        .send(SftpEvent::TransferStarted {
            kind,
            name: name.clone(),
            total,
        })
        .await
        .is_err()
    {
        return;
    }
    if offset > 0 {
        let _ = report_progress(events, kind, &name, offset, total).await;
    }

    let mut buffer = vec![0u8; TRANSFER_CHUNK_BYTES];
    let mut transferred = offset;
    let mut last_report = Instant::now();
    loop {
        if cancellation.load(Ordering::SeqCst) {
            drop(local_file);
            if offset == 0 {
                let _ = tokio::fs::remove_file(&local).await;
            }
            let _ = events
                .send(SftpEvent::TransferCancelled { kind, name })
                .await;
            return;
        }
        let read = match remote_file.read(&mut buffer).await {
            Ok(read) => read,
            Err(error) => {
                drop(local_file);
                if offset == 0 {
                    let _ = tokio::fs::remove_file(&local).await;
                }
                fail(
                    events,
                    kind,
                    &name,
                    format!("Could not read {remote}: {error}"),
                )
                .await;
                return;
            }
        };
        if read == 0 {
            break;
        }
        if let Err(error) = local_file.write_all(&buffer[..read]).await {
            drop(local_file);
            if offset == 0 {
                let _ = tokio::fs::remove_file(&local).await;
            }
            fail(
                events,
                kind,
                &name,
                format!("Could not write {}: {error}", local.display()),
            )
            .await;
            return;
        }
        transferred += read as u64;
        if last_report.elapsed() >= PROGRESS_INTERVAL {
            last_report = Instant::now();
            if !report_progress(events, kind, &name, transferred, total).await {
                return;
            }
        }
    }
    if let Err(error) = local_file.shutdown().await {
        drop(local_file);
        if offset == 0 {
            let _ = tokio::fs::remove_file(&local).await;
        }
        fail(
            events,
            kind,
            &name,
            format!("Could not finish {}: {error}", local.display()),
        )
        .await;
        return;
    }
    let _ = report_progress(events, kind, &name, transferred, total).await;
    let _ = events
        .send(SftpEvent::TransferFinished { kind, name })
        .await;
}

#[derive(Default)]
struct UploadPlan {
    dirs: Vec<(PathBuf, String)>,
    files: Vec<(PathBuf, String, u64)>,
    total: u64,
}

#[derive(Default)]
struct DownloadPlan {
    dirs: Vec<(String, PathBuf)>,
    files: Vec<(String, PathBuf, u64)>,
    total: u64,
}

async fn upload_directory(
    session: &SftpSession,
    local: PathBuf,
    remote: String,
    overwrite: bool,
    resume: bool,
    events: &mpsc::Sender<SftpEvent>,
    cancellation: &AtomicBool,
) {
    let kind = TransferKind::Upload;
    let name = local.file_name().map_or_else(
        || local.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let root_exists = session.try_exists(remote.clone()).await.unwrap_or(false);
    if root_exists && !overwrite && !resume {
        let _ = events
            .send(SftpEvent::NeedsOverwrite {
                kind,
                local,
                remote,
                is_directory: true,
            })
            .await;
        return;
    }
    cancellation.store(false, Ordering::SeqCst);
    let mut plan = UploadPlan::default();
    if let Err(error) = collect_upload_plan(&local, &remote, &mut plan, 0, cancellation).await {
        if cancellation.load(Ordering::SeqCst) {
            let _ = events
                .send(SftpEvent::TransferCancelled { kind, name })
                .await;
        } else {
            fail(
                events,
                kind,
                &name,
                format!("Could not read {}: {error:#}", local.display()),
            )
            .await;
        }
        return;
    }
    if events
        .send(SftpEvent::TransferStarted {
            kind,
            name: name.clone(),
            total: plan.total,
        })
        .await
        .is_err()
    {
        return;
    }

    let mut created_dirs = Vec::new();
    let mut created_files = Vec::new();
    if !root_exists {
        match session.create_dir(remote.clone()).await {
            Ok(()) => created_dirs.push(remote.clone()),
            Err(error) => {
                fail(
                    events,
                    kind,
                    &name,
                    format!("Could not create {remote}: {error}"),
                )
                .await;
                return;
            }
        }
    }
    for (_, remote_dir) in &plan.dirs {
        if remote_dir == &remote {
            continue;
        }
        if !session
            .try_exists(remote_dir.clone())
            .await
            .unwrap_or(false)
        {
            match session.create_dir(remote_dir.clone()).await {
                Ok(()) => created_dirs.push(remote_dir.clone()),
                Err(error) => {
                    cleanup_remote(session, &created_files, &created_dirs).await;
                    fail(
                        events,
                        kind,
                        &name,
                        format!("Could not create {remote_dir}: {error}"),
                    )
                    .await;
                    return;
                }
            }
        }
    }

    let mut buffer = vec![0u8; TRANSFER_CHUNK_BYTES];
    let mut transferred = 0u64;
    let mut last_report = Instant::now();
    for (local_file, remote_file, local_size) in &plan.files {
        if cancellation.load(Ordering::SeqCst) {
            cleanup_remote(session, &created_files, &created_dirs).await;
            let _ = events
                .send(SftpEvent::TransferCancelled { kind, name })
                .await;
            return;
        }
        let existed = session
            .try_exists(remote_file.clone())
            .await
            .unwrap_or(false);
        if !existed {
            created_files.push(remote_file.clone());
        }
        let remote_size = if resume && existed {
            session
                .metadata(remote_file.clone())
                .await
                .ok()
                .and_then(|metadata| metadata.size)
                .unwrap_or(0)
        } else {
            0
        };
        let offset = if resume {
            resume_offset(remote_size, *local_size)
        } else {
            0
        };
        if offset == *local_size && *local_size > 0 {
            transferred += *local_size;
            continue;
        }
        let mut local_handle = match tokio::fs::File::open(local_file).await {
            Ok(file) => file,
            Err(error) => {
                cleanup_remote(session, &created_files, &created_dirs).await;
                fail(
                    events,
                    kind,
                    &name,
                    format!("Could not open {}: {error}", local_file.display()),
                )
                .await;
                return;
            }
        };
        let mut remote_handle = if offset > 0 {
            match session
                .open_with_flags(remote_file.clone(), OpenFlags::WRITE)
                .await
            {
                Ok(file) => file,
                Err(error) => {
                    cleanup_remote(session, &created_files, &created_dirs).await;
                    fail(
                        events,
                        kind,
                        &name,
                        format!("Could not reopen {remote_file} for resume: {error}"),
                    )
                    .await;
                    return;
                }
            }
        } else {
            match session.create(remote_file.clone()).await {
                Ok(file) => file,
                Err(error) => {
                    cleanup_remote(session, &created_files, &created_dirs).await;
                    fail(
                        events,
                        kind,
                        &name,
                        format!("Could not create {remote_file}: {error}"),
                    )
                    .await;
                    return;
                }
            }
        };
        if offset > 0 {
            if let Err(error) = local_handle.seek(SeekFrom::Start(offset)).await {
                cleanup_remote(session, &created_files, &created_dirs).await;
                fail(
                    events,
                    kind,
                    &name,
                    format!(
                        "Could not seek {} for resume: {error}",
                        local_file.display()
                    ),
                )
                .await;
                return;
            }
            if let Err(error) = remote_handle.seek(SeekFrom::Start(offset)).await {
                cleanup_remote(session, &created_files, &created_dirs).await;
                fail(
                    events,
                    kind,
                    &name,
                    format!("Could not seek {remote_file} for resume: {error}"),
                )
                .await;
                return;
            }
            transferred += offset;
        }
        loop {
            let read = match local_handle.read(&mut buffer).await {
                Ok(read) => read,
                Err(error) => {
                    drop(remote_handle);
                    cleanup_remote(session, &created_files, &created_dirs).await;
                    fail(
                        events,
                        kind,
                        &name,
                        format!("Could not read {}: {error}", local_file.display()),
                    )
                    .await;
                    return;
                }
            };
            if read == 0 {
                break;
            }
            if cancellation.load(Ordering::SeqCst) {
                drop(remote_handle);
                cleanup_remote(session, &created_files, &created_dirs).await;
                let _ = events
                    .send(SftpEvent::TransferCancelled { kind, name })
                    .await;
                return;
            }
            if let Err(error) = remote_handle.write_all(&buffer[..read]).await {
                drop(remote_handle);
                cleanup_remote(session, &created_files, &created_dirs).await;
                fail(
                    events,
                    kind,
                    &name,
                    format!("Could not write {remote_file}: {error}"),
                )
                .await;
                return;
            }
            transferred += read as u64;
            if last_report.elapsed() >= PROGRESS_INTERVAL {
                last_report = Instant::now();
                if !report_progress(events, kind, &name, transferred, plan.total).await {
                    cleanup_remote(session, &created_files, &created_dirs).await;
                    return;
                }
            }
        }
        if let Err(error) = remote_handle.shutdown().await {
            cleanup_remote(session, &created_files, &created_dirs).await;
            fail(
                events,
                kind,
                &name,
                format!("Could not finish {remote_file}: {error}"),
            )
            .await;
            return;
        }
    }
    let _ = report_progress(events, kind, &name, transferred, plan.total).await;
    let _ = events
        .send(SftpEvent::TransferFinished { kind, name })
        .await;
}

async fn download_directory(
    session: &SftpSession,
    remote: String,
    local: PathBuf,
    overwrite: bool,
    resume: bool,
    events: &mpsc::Sender<SftpEvent>,
    cancellation: &AtomicBool,
) {
    let kind = TransferKind::Download;
    let name = local.file_name().map_or_else(
        || local.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let local_exists = tokio::fs::metadata(&local).await.is_ok();
    if local_exists && !overwrite && !resume {
        let _ = events
            .send(SftpEvent::NeedsOverwrite {
                kind,
                local,
                remote,
                is_directory: true,
            })
            .await;
        return;
    }
    cancellation.store(false, Ordering::SeqCst);
    let mut plan = DownloadPlan::default();
    if let Err(error) =
        collect_download_plan(session, &remote, &local, &mut plan, 0, cancellation).await
    {
        if cancellation.load(Ordering::SeqCst) {
            let _ = events
                .send(SftpEvent::TransferCancelled { kind, name })
                .await;
        } else {
            fail(
                events,
                kind,
                &name,
                format!("Could not read {remote}: {error:#}"),
            )
            .await;
        }
        return;
    }
    if events
        .send(SftpEvent::TransferStarted {
            kind,
            name: name.clone(),
            total: plan.total,
        })
        .await
        .is_err()
    {
        return;
    }

    let mut created_dirs = Vec::new();
    let mut created_files = Vec::new();
    if !local_exists {
        match tokio::fs::create_dir_all(&local).await {
            Ok(()) => created_dirs.push(local.clone()),
            Err(error) => {
                fail(
                    events,
                    kind,
                    &name,
                    format!("Could not create {}: {error}", local.display()),
                )
                .await;
                return;
            }
        }
    }
    for (_, local_dir) in &plan.dirs {
        if local_dir == &local {
            continue;
        }
        if tokio::fs::metadata(local_dir).await.is_err() {
            match tokio::fs::create_dir_all(local_dir).await {
                Ok(()) => created_dirs.push(local_dir.clone()),
                Err(error) => {
                    cleanup_local(&created_files, &created_dirs).await;
                    fail(
                        events,
                        kind,
                        &name,
                        format!("Could not create {}: {error}", local_dir.display()),
                    )
                    .await;
                    return;
                }
            }
        }
    }

    let mut buffer = vec![0u8; TRANSFER_CHUNK_BYTES];
    let mut transferred = 0u64;
    let mut last_report = Instant::now();
    for (remote_file, local_file, remote_size) in &plan.files {
        if cancellation.load(Ordering::SeqCst) {
            cleanup_local(&created_files, &created_dirs).await;
            let _ = events
                .send(SftpEvent::TransferCancelled { kind, name })
                .await;
            return;
        }
        let existed = tokio::fs::metadata(local_file).await.is_ok();
        if !existed {
            created_files.push(local_file.clone());
        }
        let local_size = if resume && existed {
            tokio::fs::metadata(local_file)
                .await
                .map_or(0, |metadata| metadata.len())
        } else {
            0
        };
        let offset = if resume {
            resume_offset(local_size, *remote_size)
        } else {
            0
        };
        if offset == *remote_size && *remote_size > 0 {
            transferred += *remote_size;
            continue;
        }
        let mut remote_handle = match session.open(remote_file.clone()).await {
            Ok(file) => file,
            Err(error) => {
                cleanup_local(&created_files, &created_dirs).await;
                fail(
                    events,
                    kind,
                    &name,
                    format!("Could not open {remote_file}: {error}"),
                )
                .await;
                return;
            }
        };
        let mut local_handle = if offset > 0 {
            match tokio::fs::OpenOptions::new()
                .append(true)
                .open(local_file)
                .await
            {
                Ok(file) => file,
                Err(error) => {
                    cleanup_local(&created_files, &created_dirs).await;
                    fail(
                        events,
                        kind,
                        &name,
                        format!(
                            "Could not reopen {} for resume: {error}",
                            local_file.display()
                        ),
                    )
                    .await;
                    return;
                }
            }
        } else {
            match tokio::fs::File::create(local_file).await {
                Ok(file) => file,
                Err(error) => {
                    cleanup_local(&created_files, &created_dirs).await;
                    fail(
                        events,
                        kind,
                        &name,
                        format!("Could not create {}: {error}", local_file.display()),
                    )
                    .await;
                    return;
                }
            }
        };
        if offset > 0 {
            if let Err(error) = remote_handle.seek(SeekFrom::Start(offset)).await {
                cleanup_local(&created_files, &created_dirs).await;
                fail(
                    events,
                    kind,
                    &name,
                    format!("Could not seek {remote_file} for resume: {error}"),
                )
                .await;
                return;
            }
            transferred += offset;
        }
        loop {
            let read = match remote_handle.read(&mut buffer).await {
                Ok(read) => read,
                Err(error) => {
                    drop(local_handle);
                    cleanup_local(&created_files, &created_dirs).await;
                    fail(
                        events,
                        kind,
                        &name,
                        format!("Could not read {remote_file}: {error}"),
                    )
                    .await;
                    return;
                }
            };
            if read == 0 {
                break;
            }
            if cancellation.load(Ordering::SeqCst) {
                drop(local_handle);
                cleanup_local(&created_files, &created_dirs).await;
                let _ = events
                    .send(SftpEvent::TransferCancelled { kind, name })
                    .await;
                return;
            }
            if let Err(error) = local_handle.write_all(&buffer[..read]).await {
                drop(local_handle);
                cleanup_local(&created_files, &created_dirs).await;
                fail(
                    events,
                    kind,
                    &name,
                    format!("Could not write {}: {error}", local_file.display()),
                )
                .await;
                return;
            }
            transferred += read as u64;
            if last_report.elapsed() >= PROGRESS_INTERVAL {
                last_report = Instant::now();
                if !report_progress(events, kind, &name, transferred, plan.total).await {
                    cleanup_local(&created_files, &created_dirs).await;
                    return;
                }
            }
        }
        if let Err(error) = local_handle.shutdown().await {
            cleanup_local(&created_files, &created_dirs).await;
            fail(
                events,
                kind,
                &name,
                format!("Could not finish {}: {error}", local_file.display()),
            )
            .await;
            return;
        }
    }
    let _ = report_progress(events, kind, &name, transferred, plan.total).await;
    let _ = events
        .send(SftpEvent::TransferFinished { kind, name })
        .await;
}

async fn collect_upload_plan(
    local_dir: &Path,
    remote_dir: &str,
    plan: &mut UploadPlan,
    depth: usize,
    cancellation: &AtomicBool,
) -> Result<()> {
    if depth > MAX_RECURSIVE_DEPTH {
        anyhow::bail!("directory nesting exceeds {MAX_RECURSIVE_DEPTH} levels");
    }
    if plan.files.len() + plan.dirs.len() > MAX_RECURSIVE_ENTRIES {
        anyhow::bail!("directory contains more than {MAX_RECURSIVE_ENTRIES} entries");
    }
    if cancellation.load(Ordering::SeqCst) {
        anyhow::bail!("transfer cancelled");
    }
    plan.dirs
        .push((local_dir.to_path_buf(), remote_dir.to_owned()));
    let mut reader = tokio::fs::read_dir(local_dir)
        .await
        .with_context(|| format!("Could not list {}", local_dir.display()))?;
    while let Some(entry) = reader.next_entry().await? {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.is_empty() {
            continue;
        }
        let file_type = entry.file_type().await?;
        let remote_path = remote_join(remote_dir, &name);
        if file_type.is_dir() {
            Box::pin(collect_upload_plan(
                &entry.path(),
                &remote_path,
                plan,
                depth + 1,
                cancellation,
            ))
            .await?;
        } else if file_type.is_file() {
            let size = entry.metadata().await.map_or(0, |metadata| metadata.len());
            plan.files.push((entry.path(), remote_path, size));
            plan.total = plan.total.saturating_add(size);
        }
    }
    Ok(())
}

async fn collect_download_plan(
    session: &SftpSession,
    remote_dir: &str,
    local_dir: &Path,
    plan: &mut DownloadPlan,
    depth: usize,
    cancellation: &AtomicBool,
) -> Result<()> {
    if depth > MAX_RECURSIVE_DEPTH {
        anyhow::bail!("directory nesting exceeds {MAX_RECURSIVE_DEPTH} levels");
    }
    if plan.files.len() + plan.dirs.len() > MAX_RECURSIVE_ENTRIES {
        anyhow::bail!("directory contains more than {MAX_RECURSIVE_ENTRIES} entries");
    }
    if cancellation.load(Ordering::SeqCst) {
        anyhow::bail!("transfer cancelled");
    }
    plan.dirs
        .push((remote_dir.to_owned(), local_dir.to_path_buf()));
    let entries = session
        .read_dir(remote_dir.to_owned())
        .await
        .with_context(|| format!("Could not list {remote_dir}"))?;
    for entry in entries {
        let name = entry.file_name();
        if name == "." || name == ".." {
            continue;
        }
        let local_name = safe_local_component(&name)
            .ok_or_else(|| anyhow::anyhow!("unsafe remote file name {name:?}"))?;
        let remote_path = entry.path();
        let local_path = local_dir.join(local_name);
        let file_type = entry.file_type();
        if file_type.is_dir() {
            Box::pin(collect_download_plan(
                session,
                &remote_path,
                &local_path,
                plan,
                depth + 1,
                cancellation,
            ))
            .await?;
        } else if file_type.is_file() {
            let size = entry.metadata().size.unwrap_or(0);
            plan.files.push((remote_path, local_path, size));
            plan.total = plan.total.saturating_add(size);
        }
    }
    Ok(())
}

fn safe_local_component(name: &str) -> Option<String> {
    if name == "." || name == ".." {
        return None;
    }
    let component = Path::new(name).file_name()?.to_str()?;
    (component == name).then(|| component.to_owned())
}

async fn cleanup_remote(session: &SftpSession, files: &[String], dirs: &[String]) {
    for file in files.iter().rev() {
        let _ = session.remove_file(file.clone()).await;
    }
    for dir in dirs.iter().rev() {
        let _ = session.remove_dir(dir.clone()).await;
    }
}

async fn cleanup_local(files: &[PathBuf], dirs: &[PathBuf]) {
    for file in files.iter().rev() {
        let _ = tokio::fs::remove_file(file).await;
    }
    for dir in dirs.iter().rev() {
        let _ = tokio::fs::remove_dir(dir).await;
    }
}

async fn report_progress(
    events: &mpsc::Sender<SftpEvent>,
    kind: TransferKind,
    name: &str,
    transferred: u64,
    total: u64,
) -> bool {
    events
        .send(SftpEvent::TransferProgress {
            kind,
            name: name.to_owned(),
            transferred,
            total,
        })
        .await
        .is_ok()
}

async fn fail(events: &mpsc::Sender<SftpEvent>, kind: TransferKind, name: &str, error: String) {
    let _ = events
        .send(SftpEvent::TransferFailed {
            kind,
            name: name.to_owned(),
            error,
        })
        .await;
}

pub fn remote_join(base: &str, name: &str) -> String {
    if base.is_empty() || base == "/" {
        format!("/{name}")
    } else {
        format!("{}/{name}", base.trim_end_matches('/'))
    }
}

pub fn parent_remote_path(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    let Some(index) = trimmed.rfind('/') else {
        return "/".to_owned();
    };
    if index == 0 {
        "/".to_owned()
    } else {
        trimmed[..index].to_owned()
    }
}

pub fn permission_mode(permissions: Option<u32>) -> u32 {
    permissions.unwrap_or_default() & 0o7777
}

pub fn format_permission_mode(permissions: Option<u32>) -> String {
    format!("{:04o}", permission_mode(permissions))
}

pub fn parse_permission_mode(input: &str) -> Result<u32, &'static str> {
    let value = input.trim();
    if !(3..=4).contains(&value.len()) || !value.bytes().all(|byte| (b'0'..=b'7').contains(&byte)) {
        return Err("permissions must be a 3- or 4-digit octal value");
    }
    let mode = u32::from_str_radix(value, 8).map_err(|_| "permissions must be octal")?;
    if mode > 0o7777 {
        return Err("permissions must be at most 07777");
    }
    Ok(mode)
}

pub fn parse_unix_timestamp(input: &str) -> Result<Option<u32>, &'static str> {
    let value = input.trim();
    if value.is_empty() {
        return Ok(None);
    }
    let timestamp = value
        .parse::<u64>()
        .map_err(|_| "timestamps must be unsigned Unix seconds")?;
    u32::try_from(timestamp)
        .map(Some)
        .map_err(|_| "timestamps must fit in an unsigned 32-bit value")
}

#[cfg(test)]
mod tests {
    use super::{
        format_permission_mode, parent_remote_path, parse_permission_mode, parse_unix_timestamp,
        remote_join, resume_offset,
    };

    #[test]
    fn resume_offsets_continue_partial_files_and_restart_invalid_ones() {
        assert_eq!(resume_offset(0, 100), 0);
        assert_eq!(resume_offset(40, 100), 40);
        assert_eq!(resume_offset(100, 100), 100);
        assert_eq!(resume_offset(120, 100), 0);
        assert_eq!(resume_offset(0, 0), 0);
    }

    #[test]
    fn remote_paths_join_and_climb_without_double_slashes() {
        assert_eq!(
            remote_join("/home/operator", "notes.txt"),
            "/home/operator/notes.txt"
        );
        assert_eq!(remote_join("/", "etc"), "/etc");
        assert_eq!(remote_join("/home/operator/", "src"), "/home/operator/src");
        assert_eq!(
            parent_remote_path("/home/operator/notes.txt"),
            "/home/operator"
        );
        assert_eq!(parent_remote_path("/home/operator/"), "/home");
        assert_eq!(parent_remote_path("/etc"), "/");
        assert_eq!(parent_remote_path("/"), "/");
        assert_eq!(parent_remote_path("relative"), "/");
    }

    #[test]
    fn permission_modes_parse_and_format_as_octal() {
        assert_eq!(parse_permission_mode("755"), Ok(0o755));
        assert_eq!(parse_permission_mode("0755"), Ok(0o755));
        assert_eq!(format_permission_mode(Some(0o100755)), "0755");
        assert_eq!(format_permission_mode(None), "0000");
        assert!(parse_permission_mode("888").is_err());
        assert!(parse_permission_mode("17777").is_err());
    }

    #[test]
    fn unix_timestamps_accept_empty_and_unsigned_32_bit_values() {
        assert_eq!(parse_unix_timestamp(""), Ok(None));
        assert_eq!(parse_unix_timestamp("0"), Ok(Some(0)));
        assert_eq!(parse_unix_timestamp("4294967295"), Ok(Some(u32::MAX)));
        assert!(parse_unix_timestamp("-1").is_err());
        assert!(parse_unix_timestamp("4294967296").is_err());
    }
}
