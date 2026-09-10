//! The export job the frame owns, and the background worker behind it.
//!
//! Moved out of `tabs/mod.rs` unchanged. `pub(super)` here reaches
//! `chrome::tabs` and everything under it, which is the scope these items had
//! while they were private items of `chrome::tabs`.

use std::collections::HashSet;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;

use gpui::{App, AppContext as _, Context, Entity, EntityId, Timer};
use onionskin_plugin_api::{ExportOutputKind, PageIndex};
use tempfile::{NamedTempFile, TempDir};

use super::{tab_title, ShellFrame};
use crate::shell::canvas::{CanvasError, PreparedExport};
use crate::shell::chrome::global_bar::ExportTarget;
use crate::shell::{Canvas, POLL_INTERVAL};

impl ShellFrame {
    /// Ask where the export goes, then install one background job.
    pub(super) fn start_export(&mut self, target: ExportTarget, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.active() else {
            return;
        };
        let canvas = tab.canvas.clone();
        if self.export.export_job.is_some() {
            report_export_failure(&canvas, ExportFailure::AlreadyInProgress, cx);
            return;
        }
        let origin = canvas.entity_id();
        let extension = canvas
            .read(cx)
            .model
            .registry()
            .codec(target.codec())
            .map(|codec| codec.extension());
        let Some(extension) = extension else {
            report_export_failure(&canvas, CanvasError::UnknownCodec(target.codec()), cx);
            return;
        };
        let directory = tab
            .source
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
        let suggested = format!("{}.{extension}", tab_title(&tab.source));
        let chosen = cx.prompt_for_new_path(&directory, Some(&suggested));

        cx.spawn(async move |frame, cx| {
            let path = match chosen.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    frame
                        .update(cx, |frame, cx| {
                            if frame.is_active_canvas(origin) {
                                frame
                                    .notices
                                    .push(format!("no destination could be chosen: {error}"));
                                cx.notify();
                            }
                        })
                        .ok();
                    return;
                }
            };
            frame
                .update(cx, |frame, cx| {
                    if frame.is_active_canvas(origin) {
                        if frame.export.export_job.is_some() {
                            report_export_failure(&canvas, ExportFailure::AlreadyInProgress, cx);
                            return;
                        }
                        let prepared = canvas.update(cx, |canvas, _cx| {
                            canvas.model.prepare_export(target.codec(), EXPORT_DPI)
                        });
                        match prepared {
                            Ok(prepared) => {
                                frame.launch_export(origin, canvas.clone(), path, prepared, cx)
                            }
                            Err(error) => report_export_failure(&canvas, error, cx),
                        }
                    }
                })
                .ok();
        })
        .detach();
    }

    pub(super) fn launch_export(
        &mut self,
        origin: EntityId,
        canvas: Entity<Canvas>,
        path: PathBuf,
        prepared: PreparedExport,
        cx: &mut Context<Self>,
    ) {
        let id = self.export.next_export_id;
        self.export.next_export_id = self.export.next_export_id.wrapping_add(1);
        let phase = Arc::new(ExportPhase::new());
        let completed = Arc::new(AtomicUsize::new(0));
        let total = prepared.request.pages.pages().count();
        self.export.export_job = Some(ExportJob {
            id,
            origin,
            phase: Arc::clone(&phase),
            completed: Arc::clone(&completed),
            total,
            last_displayed: 0,
        });
        let task =
            cx.background_spawn(
                async move { run_export_worker(prepared, &path, &phase, &completed) },
            );
        cx.spawn(async move |frame, cx| {
            let result = task.await;
            _ = frame.update(cx, |frame, cx| {
                frame.finish_export(id, origin, &canvas, result, cx);
            });
        })
        .detach();
        self.arm_export_poll(id, cx);
        cx.notify();
    }

    pub(super) fn arm_export_poll(&mut self, id: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |frame, cx| loop {
            Timer::after(POLL_INTERVAL).await;
            let keep_polling = frame
                .update(cx, |frame, cx| {
                    let (keep_polling, changed) = frame.poll_export_progress(id);
                    if changed {
                        cx.notify();
                    }
                    keep_polling
                })
                .unwrap_or(false);
            if !keep_polling {
                break;
            }
        })
        .detach();
    }

    pub(super) fn poll_export_progress(&mut self, id: u64) -> (bool, bool) {
        let Some(job) = self.export.export_job.as_mut().filter(|job| job.id == id) else {
            return (false, false);
        };
        let completed = job.completed.load(Ordering::Acquire);
        let changed = completed != job.last_displayed;
        if changed {
            job.last_displayed = completed;
        }
        (true, changed)
    }

    pub(super) fn finish_export(
        &mut self,
        id: u64,
        origin: EntityId,
        canvas: &Entity<Canvas>,
        result: Result<ExportOutcome, ExportFailure>,
        cx: &mut Context<Self>,
    ) {
        if !matches!(self.export.export_job.as_ref(), Some(job) if job.id == id) {
            return;
        }
        self.export.export_job = None;
        if let Err(error) = result {
            if self
                .tabs
                .tabs()
                .iter()
                .any(|tab| tab.canvas.entity_id() == origin)
            {
                report_export_failure(canvas, error, cx);
            }
        }
        cx.notify();
    }

    pub(super) fn cancel_export(&mut self, cx: &mut Context<Self>) {
        if self
            .export
            .export_job
            .as_ref()
            .is_some_and(|job| job.phase.cancel())
        {
            cx.notify();
        }
    }

    pub(super) fn cancel_export_for(&mut self, origin: EntityId) {
        if let Some(job) = &self.export.export_job {
            if job.origin == origin {
                job.phase.cancel();
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(super) enum ExportPhaseValue {
    Running,
    Cancelling,
    Publishing,
}

pub(super) struct ExportPhase(AtomicU8);

impl ExportPhase {
    pub(super) fn new() -> Self {
        Self(AtomicU8::new(ExportPhaseValue::Running as u8))
    }

    pub(super) fn load(&self) -> ExportPhaseValue {
        match self.0.load(Ordering::Acquire) {
            value if value == ExportPhaseValue::Running as u8 => ExportPhaseValue::Running,
            value if value == ExportPhaseValue::Cancelling as u8 => ExportPhaseValue::Cancelling,
            value if value == ExportPhaseValue::Publishing as u8 => ExportPhaseValue::Publishing,
            // The one wildcard the split kept in a dispatch position, and the
            // one place in this module where adding a variant is not a compile
            // error: the arms are guards over a `u8`, which rustc cannot prove
            // exhaustive, so a fourth `ExportPhaseValue` would arrive here and
            // panic. Loud rather than silent, but loud at runtime.
            _ => unreachable!("export phase is written only from ExportPhaseValue"),
        }
    }

    pub(super) fn cancel(&self) -> bool {
        self.0
            .compare_exchange(
                ExportPhaseValue::Running as u8,
                ExportPhaseValue::Cancelling as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    pub(super) fn begin_publishing(&self) -> bool {
        self.0
            .compare_exchange(
                ExportPhaseValue::Running as u8,
                ExportPhaseValue::Publishing as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    pub(super) fn is_cancelling(&self) -> bool {
        self.load() == ExportPhaseValue::Cancelling
    }
}

/// The frame's export state: the one job that can be in flight, and the
/// counter that names the next one.
///
/// Declared here rather than among `ShellFrame`'s fields so that a package
/// adding to the export surface edits this file and not the frame's
/// declaration, which every other package is also editing.
#[derive(Default)]
pub(super) struct ExportState {
    pub(super) export_job: Option<ExportJob>,
    pub(super) next_export_id: u64,
}

pub(super) struct ExportJob {
    pub(super) id: u64,
    pub(super) origin: EntityId,
    pub(super) phase: Arc<ExportPhase>,
    pub(super) completed: Arc<AtomicUsize>,
    pub(super) total: usize,
    pub(super) last_displayed: usize,
}

pub(super) fn export_progress_label(job: &ExportJob) -> String {
    match job.phase.load() {
        ExportPhaseValue::Running => {
            format!("Exporting {} of {} pages", job.last_displayed, job.total)
        }
        ExportPhaseValue::Cancelling => format!(
            "Cancelling export, {} of {} pages complete",
            job.last_displayed, job.total
        ),
        ExportPhaseValue::Publishing => "Finishing export".to_owned(),
    }
}

/// Resolution for a raster export. M2 has no export-settings dialog, so this
/// is Acrobat's own default rather than a number picked here; the codec API
/// takes the resolution as an argument so the dialog that lands with M3's
/// `File > Export To` has somewhere to put the user's choice.
pub(super) const EXPORT_DPI: f32 = 150.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ExportOutcome {
    Complete,
    Cancelled,
}

pub(super) fn run_export_worker(
    prepared: PreparedExport,
    chosen: &Path,
    phase: &ExportPhase,
    completed: &AtomicUsize,
) -> Result<ExportOutcome, ExportFailure> {
    run_export_worker_observed(prepared, chosen, phase, completed, &NoopExportObserver)
}

pub(super) trait ExportObserver: Send + Sync {
    fn writer_opened(&self) {}
    fn writer_closed(&self) {}
    fn page_completed(&self, _page: PageIndex) {}
    fn before_publish(&self) {}
    fn after_publish_started(&self) {}
}

struct NoopExportObserver;

impl ExportObserver for NoopExportObserver {}

pub(super) fn run_export_worker_observed(
    prepared: PreparedExport,
    chosen: &Path,
    phase: &ExportPhase,
    completed: &AtomicUsize,
    observer: &dyn ExportObserver,
) -> Result<ExportOutcome, ExportFailure> {
    match prepared.output_kind {
        ExportOutputKind::Single => export_single(prepared, chosen, phase, completed, observer),
        ExportOutputKind::PerPage => export_per_page(prepared, chosen, phase, completed, observer),
    }
}

fn export_single(
    prepared: PreparedExport,
    chosen: &Path,
    phase: &ExportPhase,
    completed: &AtomicUsize,
    observer: &dyn ExportObserver,
) -> Result<ExportOutcome, ExportFailure> {
    let parent = chosen.parent().unwrap_or_else(|| Path::new("."));
    let mut output = NamedTempFile::new_in(parent).map_err(|source| ExportFailure::Write {
        path: chosen.to_path_buf(),
        source,
    })?;
    observer.writer_opened();
    let mut document = prepared
        .snapshot
        .open()
        .map_err(|error| ExportFailure::Codec(CanvasError::Core(error)))?;
    for (position, page) in prepared.request.pages.pages().enumerate() {
        if phase.is_cancelling() {
            return Ok(ExportOutcome::Cancelled);
        }
        let bytes = prepared
            .codec
            .export_page(&mut document, &prepared.request, page, position == 0)
            .map_err(|error| ExportFailure::Codec(CanvasError::Export(error)))?;
        if phase.is_cancelling() {
            return Ok(ExportOutcome::Cancelled);
        }
        output
            .write_all(&bytes)
            .map_err(|source| ExportFailure::Write {
                path: chosen.to_path_buf(),
                source,
            })?;
        if phase.is_cancelling() {
            return Ok(ExportOutcome::Cancelled);
        }
        output.flush().map_err(|source| ExportFailure::Write {
            path: chosen.to_path_buf(),
            source,
        })?;
        if phase.is_cancelling() {
            return Ok(ExportOutcome::Cancelled);
        }
        completed.fetch_add(1, Ordering::Release);
        observer.page_completed(page);
    }
    observer.writer_closed();
    observer.before_publish();
    if !phase.begin_publishing() {
        return Ok(ExportOutcome::Cancelled);
    }
    observer.after_publish_started();
    output.persist_noclobber(chosen).map_err(|failure| {
        if failure.error.kind() == std::io::ErrorKind::AlreadyExists {
            ExportFailure::Exists {
                path: chosen.to_path_buf(),
            }
        } else {
            ExportFailure::Write {
                path: chosen.to_path_buf(),
                source: failure.error,
            }
        }
    })?;
    Ok(ExportOutcome::Complete)
}

fn export_per_page(
    prepared: PreparedExport,
    chosen: &Path,
    phase: &ExportPhase,
    completed: &AtomicUsize,
    observer: &dyn ExportObserver,
) -> Result<ExportOutcome, ExportFailure> {
    let paths = prepared
        .request
        .pages
        .pages()
        .map(|page| export_path(chosen, Some(page), prepared.page_count))
        .collect::<Vec<_>>();
    preflight_export_paths(&paths)?;
    let parent = chosen.parent().unwrap_or_else(|| Path::new("."));
    let quarantine = tempfile::tempdir_in(parent).map_err(|source| ExportFailure::Write {
        path: chosen.to_path_buf(),
        source,
    })?;
    let mut document = prepared
        .snapshot
        .open()
        .map_err(|error| ExportFailure::Codec(CanvasError::Core(error)))?;
    let mut owned = Vec::new();
    for ((position, page), path) in prepared.request.pages.pages().enumerate().zip(paths) {
        if phase.is_cancelling() {
            return cancel_per_page(owned, quarantine);
        }
        let bytes =
            match prepared
                .codec
                .export_page(&mut document, &prepared.request, page, position == 0)
            {
                Ok(bytes) => bytes,
                Err(error) => {
                    return fail_per_page(
                        ExportFailure::Codec(CanvasError::Export(error)),
                        owned,
                        quarantine,
                    )
                }
            };
        if phase.is_cancelling() {
            return cancel_per_page(owned, quarantine);
        }
        let mut writer = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(writer) => writer,
            Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
                return fail_per_page(ExportFailure::Exists { path }, owned, quarantine)
            }
            Err(source) => {
                return fail_per_page(ExportFailure::Write { path, source }, owned, quarantine)
            }
        };
        observer.writer_opened();
        let identity = match FileIdentity::from_file(&writer) {
            Ok(identity) => identity,
            Err(source) => {
                drop(writer);
                observer.writer_closed();
                owned.push(OwnedExport {
                    path,
                    identity: None,
                });
                return fail_per_page(
                    ExportFailure::Write {
                        path: chosen.to_path_buf(),
                        source,
                    },
                    owned,
                    quarantine,
                );
            }
        };
        owned.push(OwnedExport {
            path: path.clone(),
            identity: Some(identity),
        });
        if phase.is_cancelling() {
            drop(writer);
            observer.writer_closed();
            return cancel_per_page(owned, quarantine);
        }
        if let Err(source) = writer.write_all(&bytes) {
            drop(writer);
            observer.writer_closed();
            return fail_per_page(ExportFailure::Write { path, source }, owned, quarantine);
        }
        if phase.is_cancelling() {
            drop(writer);
            observer.writer_closed();
            return cancel_per_page(owned, quarantine);
        }
        if let Err(source) = writer.flush() {
            drop(writer);
            observer.writer_closed();
            return fail_per_page(ExportFailure::Write { path, source }, owned, quarantine);
        }
        if phase.is_cancelling() {
            drop(writer);
            observer.writer_closed();
            return cancel_per_page(owned, quarantine);
        }
        drop(writer);
        observer.writer_closed();
        completed.fetch_add(1, Ordering::Release);
        observer.page_completed(page);
    }
    observer.before_publish();
    if !phase.begin_publishing() {
        return cancel_per_page(owned, quarantine);
    }
    observer.after_publish_started();
    Ok(ExportOutcome::Complete)
}

pub(super) fn preflight_export_paths(paths: &[PathBuf]) -> Result<(), ExportFailure> {
    let mut seen = HashSet::new();
    for path in paths {
        if !seen.insert(path.clone()) {
            return Err(ExportFailure::Duplicate { path: path.clone() });
        }
        match std::fs::symlink_metadata(path) {
            Ok(_) => return Err(ExportFailure::Exists { path: path.clone() }),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(ExportFailure::Write {
                    path: path.clone(),
                    source,
                })
            }
        }
    }
    Ok(())
}

fn cancel_per_page(
    paths: Vec<OwnedExport>,
    quarantine: TempDir,
) -> Result<ExportOutcome, ExportFailure> {
    match remove_export_paths(paths, quarantine) {
        Ok(()) => Ok(ExportOutcome::Cancelled),
        Err(cleanup) => Err(ExportFailure::Cleanup {
            primary: Box::new(ExportFailure::Cancelled),
            path: cleanup.path,
            source: cleanup.source,
        }),
    }
}

fn fail_per_page(
    failure: ExportFailure,
    paths: Vec<OwnedExport>,
    quarantine: TempDir,
) -> Result<ExportOutcome, ExportFailure> {
    match remove_export_paths(paths, quarantine) {
        Ok(()) => Err(failure),
        Err(cleanup) => Err(ExportFailure::Cleanup {
            primary: Box::new(failure),
            path: cleanup.path,
            source: cleanup.source,
        }),
    }
}

struct OwnedExport {
    path: PathBuf,
    identity: Option<FileIdentity>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    volume: u64,
    file: u64,
}

impl FileIdentity {
    #[cfg(unix)]
    fn from_file(file: &File) -> std::io::Result<Self> {
        Self::from_metadata(&file.metadata()?)
    }

    #[cfg(unix)]
    fn from_metadata(metadata: &std::fs::Metadata) -> std::io::Result<Self> {
        use std::os::unix::fs::MetadataExt as _;

        Ok(Self {
            volume: metadata.dev(),
            file: metadata.ino(),
        })
    }

    #[cfg(windows)]
    fn from_file(file: &File) -> std::io::Result<Self> {
        Self::from_metadata(&file.metadata()?)
    }

    #[cfg(windows)]
    fn from_metadata(metadata: &std::fs::Metadata) -> std::io::Result<Self> {
        use std::os::windows::fs::MetadataExt as _;

        Ok(Self {
            volume: u64::from(
                metadata
                    .volume_serial_number()
                    .ok_or_else(|| std::io::Error::other("volume identity is unavailable"))?,
            ),
            file: metadata
                .file_index()
                .ok_or_else(|| std::io::Error::other("file identity is unavailable"))?,
        })
    }

    #[cfg(not(any(unix, windows)))]
    fn from_file(_file: &File) -> std::io::Result<Self> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "safe export rollback is unsupported on this platform",
        ))
    }

    #[cfg(not(any(unix, windows)))]
    fn from_metadata(_metadata: &std::fs::Metadata) -> std::io::Result<Self> {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "safe export rollback is unsupported on this platform",
        ))
    }

    fn from_path(path: &Path) -> std::io::Result<Self> {
        Self::from_metadata(&std::fs::symlink_metadata(path)?)
    }
}

fn remove_export_paths(
    paths: Vec<OwnedExport>,
    quarantine: TempDir,
) -> Result<(), ExportCleanupFailure> {
    let mut first_error = None;
    for (index, owned) in paths.into_iter().enumerate() {
        let quarantine_path = quarantine.path().join(index.to_string());
        let result = remove_owned_export(&owned, &quarantine_path);
        if let Err(source) = result {
            first_error.get_or_insert(ExportCleanupFailure {
                path: owned.path,
                source,
            });
        }
    }
    match first_error {
        Some(error) => {
            let _ = quarantine.keep();
            Err(error)
        }
        None => Ok(()),
    }
}

fn remove_owned_export(owned: &OwnedExport, quarantine: &Path) -> std::io::Result<()> {
    let Some(expected) = owned.identity else {
        return Err(std::io::Error::other(
            "export file identity was unavailable",
        ));
    };
    match std::fs::rename(&owned.path, quarantine) {
        Ok(()) => {}
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(source) => return Err(source),
    }
    match FileIdentity::from_path(quarantine) {
        Ok(actual) if actual == expected => std::fs::remove_file(quarantine),
        Ok(_) => restore_quarantined_export(quarantine, &owned.path).map_err(|source| {
            std::io::Error::new(
                source.kind(),
                format!("replacement was preserved in {}: {source}", quarantine.display()),
            )
        }),
        Err(identity_error) => match restore_quarantined_export(quarantine, &owned.path) {
            Ok(()) => Err(identity_error),
            Err(restore_error) => Err(std::io::Error::new(
                restore_error.kind(),
                format!(
                    "identity check failed ({identity_error}); file was preserved in {} but could not be restored: {restore_error}",
                    quarantine.display()
                ),
            )),
        },
    }
}

fn restore_quarantined_export(quarantine: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::hard_link(quarantine, destination)?;
    std::fs::remove_file(quarantine)
}

#[derive(Debug)]
struct ExportCleanupFailure {
    path: PathBuf,
    source: std::io::Error,
}

/// Where one exported file goes. A single file takes the name the user chose;
/// a per-page export numbers beside it, one-based like the page controls, so
/// `report.png` becomes `report-01.png`, `report-02.png`.
pub(super) fn export_path(chosen: &Path, page: Option<PageIndex>, count: usize) -> PathBuf {
    let (Some(page), true) = (page, count > 1) else {
        return chosen.to_path_buf();
    };
    let stem = chosen
        .file_stem()
        .map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    let number = page + 1;
    let width = count.to_string().len().max(2);
    let mut name = format!("{stem}-{number:0width$}");
    if let Some(extension) = chosen.extension() {
        name.push('.');
        name.push_str(&extension.to_string_lossy());
    }
    chosen.with_file_name(name)
}

/// Everything that can go wrong once the user has chosen a destination.
///
/// A write names the file it was on. A per-page export is many files, so
/// "export could not be written" alone would not tell the user which of them
/// to look for, nor how far the set got.
#[derive(Debug)]
pub(super) enum ExportFailure {
    AlreadyInProgress,
    Cancelled,
    Codec(CanvasError),
    Exists {
        path: PathBuf,
    },
    Duplicate {
        path: PathBuf,
    },
    Write {
        path: PathBuf,
        source: std::io::Error,
    },
    Cleanup {
        primary: Box<ExportFailure>,
        path: PathBuf,
        source: std::io::Error,
    },
}

impl fmt::Display for ExportFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyInProgress => write!(f, "an export is already in progress"),
            Self::Cancelled => write!(f, "export cancelled"),
            Self::Codec(error) => write!(f, "export failed: {error}"),
            Self::Exists { path } => write!(
                f,
                "{} already exists; choose another export name",
                path.display()
            ),
            Self::Duplicate { path } => write!(
                f,
                "{} would be written more than once; choose another export name",
                path.display()
            ),
            Self::Write { path, source } => {
                write!(f, "{} could not be written: {source}", path.display())
            }
            Self::Cleanup {
                primary,
                path,
                source,
            } => write!(
                f,
                "{primary}; additionally {} could not be removed: {source}",
                path.display()
            ),
        }
    }
}

/// Surface the failure on the document it belongs to, rather than only on
/// stderr where a user will never see it.
pub(super) fn report_export_failure(
    canvas: &Entity<Canvas>,
    failure: impl fmt::Display,
    cx: &mut App,
) {
    canvas.update(cx, |canvas, cx| {
        if canvas.model.record_error(failure) {
            cx.notify();
        }
    });
}
impl Drop for ShellFrame {
    fn drop(&mut self) {
        if let Some(job) = &self.export.export_job {
            job.phase.cancel();
        }
    }
}
