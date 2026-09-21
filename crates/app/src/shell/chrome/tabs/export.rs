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

use gpui::{App, AppContext as _, Context, Entity, EntityId, Focusable as _, Timer, Window};
use onionskin_plugin_api::{ExportOutputKind, ExportRequest, PageIndex};
use tempfile::{NamedTempFile, TempDir};

use super::{tab_title, ShellFrame};
use crate::shell::canvas::{CanvasError, PreparedExport};
use crate::shell::chrome::export_dialog::{ExportDialogState, ValidationError};
use crate::shell::chrome::global_bar::ExportTarget;
use crate::shell::dialog::ShellDialog;
use crate::shell::{Canvas, POLL_INTERVAL};

impl ShellFrame {
    pub(super) fn start_export(
        &mut self,
        target: ExportTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.active() else {
            return;
        };
        let canvas = tab.canvas.clone();
        if self.export.export_job.is_some() {
            report_export_failure(&canvas, ExportFailure::AlreadyInProgress, cx);
            return;
        }
        let origin = canvas.entity_id();
        let has_codec = canvas
            .read(cx)
            .model
            .registry()
            .codec(target.codec())
            .is_some();
        if !has_codec {
            report_export_failure(&canvas, CanvasError::UnknownCodec(target.codec()), cx);
            return;
        }
        let page_count = canvas.read(cx).model.view_state().page_count;
        self.close_dialog(window, cx);
        let dialog = ExportDialogState::new(
            target,
            origin,
            page_count,
            self.shell_view_state.tokens(),
            cx,
        );
        for input in [&dialog.first, &dialog.last, &dialog.dpi, &dialog.quality] {
            let mut previous = input.read(cx).query().to_owned();
            cx.observe(input, move |frame, input, cx| {
                let query = input.read(cx).query();
                if query != previous {
                    previous = query.to_owned();
                    if let Some(dialog) = frame.export.dialog.as_mut() {
                        dialog.error = None;
                        cx.notify();
                    }
                }
            })
            .detach();
        }
        window.focus(&dialog.first.read(cx).focus_handle(cx));
        self.export.dialog = Some(dialog);
        self.dialog = Some(ShellDialog::Export);
        self.dismiss_menus(cx);
        cx.notify();
    }

    pub(in crate::shell) fn export_dialog(&self) -> Option<&ExportDialogState> {
        self.export.dialog.as_ref()
    }

    pub(in crate::shell) fn submit_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.export.dialog.as_ref() else {
            return;
        };
        let origin = dialog.origin;
        if !self.is_active_canvas(origin) {
            self.close_dialog(window, cx);
            return;
        }
        if self.export.export_job.is_some() {
            self.export.dialog.as_mut().expect("dialog exists").error =
                Some(ValidationError::AlreadyInProgress);
            cx.notify();
            return;
        }
        let request = match dialog.request(cx) {
            Ok(request) => request,
            Err(error) => {
                self.export.dialog.as_mut().expect("dialog exists").error = Some(error);
                cx.notify();
                return;
            }
        };
        let target = dialog.target;
        self.close_dialog(window, cx);
        self.prompt_export(target, origin, request, cx);
    }

    fn prompt_export(
        &mut self,
        target: ExportTarget,
        origin: EntityId,
        request: ExportRequest,
        cx: &mut Context<Self>,
    ) {
        let Some(tab) = self.tabs.active() else {
            return;
        };
        let canvas = tab.canvas.clone();
        let Some(codec) = canvas.read(cx).model.registry().codec(target.codec()) else {
            report_export_failure(&canvas, CanvasError::UnknownCodec(target.codec()), cx);
            return;
        };
        let extension = codec.extension();
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
                            canvas.model.prepare_export(target.codec(), request)
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
#[derive(Default)]
pub(super) struct ExportState {
    pub(super) dialog: Option<ExportDialogState>,
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

#[cfg(all(test, feature = "shell-test-support"))]
pub(super) const EXPORT_DPI: f32 = crate::shell::chrome::export_dialog::DEFAULT_EXPORT_DPI;

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

#[cfg(test)]
mod tests {
    use super::super::export::{
        export_path, preflight_export_paths, run_export_worker, run_export_worker_observed,
        ExportFailure, ExportObserver, ExportOutcome, ExportPhase,
    };
    #[cfg(feature = "shell-test-support")]
    use super::super::export::{report_export_failure, ExportJob, EXPORT_DPI};
    use super::super::tests::BlockingCodec;
    #[cfg(feature = "shell-test-support")]
    use super::super::tests::{bound_window, bound_window_from_bytes, install_test_export_job};
    #[cfg(feature = "shell-test-support")]
    use super::super::{Activation, App, Canvas, Entity, PaneAction, TabCommand, ViewSize};
    use super::super::{Document, ExportPhaseValue};
    #[cfg(feature = "shell-test-support")]
    use crate::preferences::ThemePreference;
    use crate::shell::canvas::PreparedExport;
    #[cfg(feature = "shell-test-support")]
    use crate::shell::canvas::{CanvasModel, CanvasStatus};
    #[cfg(feature = "shell-test-support")]
    use crate::shell::chrome::global_bar::ExportTarget;
    #[cfg(feature = "shell-test-support")]
    use crate::shell::chrome::theme::ShellViewState;
    #[cfg(feature = "shell-test-support")]
    use crate::shell::panes;
    #[cfg(feature = "shell-test-support")]
    use crate::shell::panes::NavigationPane;
    #[cfg(feature = "shell-test-support")]
    use gpui::{TestAppContext, VisualTestContext};
    #[cfg(feature = "shell-test-support")]
    use onionskin_plugin_api::PluginRegistry;
    use onionskin_plugin_api::{
        CodecPlugin, ExportError, ExportOutputKind, ExportRequest, PageIndex, PageRange,
    };
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{mpsc, Arc, Condvar, Mutex};

    #[cfg(feature = "shell-test-support")]
    fn run_export(canvas: &Entity<Canvas>, target: ExportTarget, path: &Path, cx: &mut App) {
        let prepared = canvas.update(cx, |canvas, _cx| {
            canvas.model.prepare_export(
                target.codec(),
                ExportRequest {
                    pages: PageRange::whole(canvas.model.view_state().page_count).unwrap(),
                    dpi: EXPORT_DPI,
                    quality: None,
                },
            )
        });
        let result = match prepared {
            Ok(prepared) => {
                run_export_worker(prepared, path, &ExportPhase::new(), &AtomicUsize::new(0))
            }
            Err(error) => Err(ExportFailure::Codec(error)),
        };
        if let Err(error) = result {
            report_export_failure(canvas, error, cx);
        }
    }

    struct WorkerCodec {
        kind: ExportOutputKind,
        calls: Arc<Mutex<Vec<PageIndex>>>,
        fail_on: Option<PageIndex>,
        cancel_on: Option<(PageIndex, Arc<ExportPhase>)>,
        on_page: Option<Arc<dyn Fn(PageIndex) + Send + Sync>>,
    }

    #[derive(Default)]
    struct RecordingExportObserver {
        open_writers: AtomicUsize,
        max_open_writers: AtomicUsize,
        before_publish: Option<Arc<dyn Fn() + Send + Sync>>,
        after_publish_started: Option<Arc<dyn Fn() + Send + Sync>>,
        page_completed: Option<Arc<dyn Fn(PageIndex) + Send + Sync>>,
    }

    impl ExportObserver for RecordingExportObserver {
        fn writer_opened(&self) {
            let open = self.open_writers.fetch_add(1, Ordering::AcqRel) + 1;
            self.max_open_writers.fetch_max(open, Ordering::AcqRel);
        }

        fn writer_closed(&self) {
            self.open_writers.fetch_sub(1, Ordering::AcqRel);
        }

        fn page_completed(&self, page: PageIndex) {
            if let Some(callback) = &self.page_completed {
                callback(page);
            }
        }

        fn before_publish(&self) {
            if let Some(callback) = &self.before_publish {
                callback();
            }
        }

        fn after_publish_started(&self) {
            if let Some(callback) = &self.after_publish_started {
                callback();
            }
        }
    }

    impl CodecPlugin for WorkerCodec {
        fn id(&self) -> &'static str {
            "worker-test"
        }

        fn name(&self) -> &'static str {
            "Worker Test"
        }

        fn extension(&self) -> &'static str {
            "test"
        }

        fn output_kind(&self) -> ExportOutputKind {
            self.kind
        }

        fn export_page(
            &self,
            _doc: &mut Document,
            _request: &ExportRequest,
            page: PageIndex,
            _first_in_request: bool,
        ) -> Result<Vec<u8>, ExportError> {
            self.calls.lock().unwrap().push(page);
            if let Some(on_page) = &self.on_page {
                on_page(page);
            }
            if let Some((cancel_page, phase)) = &self.cancel_on {
                if *cancel_page == page {
                    phase.cancel();
                }
            }
            if self.fail_on == Some(page) {
                return Err(ExportError::Encode {
                    page,
                    source: Box::new(std::io::Error::other("codec failed")),
                });
            }
            Ok(format!("page-{page}").into_bytes())
        }
    }

    fn prepared_worker_export(
        kind: ExportOutputKind,
        calls: Arc<Mutex<Vec<PageIndex>>>,
        fail_on: Option<PageIndex>,
        cancel_on: Option<(PageIndex, Arc<ExportPhase>)>,
        page_count: usize,
    ) -> PreparedExport {
        let document = Document::open_bytes(crate::shell::fixtures::many_pages_pdf(page_count))
            .expect("the fixture opens");
        PreparedExport {
            snapshot: document.export_snapshot().expect("the snapshot prepares"),
            codec: Arc::new(WorkerCodec {
                kind,
                calls,
                fail_on,
                cancel_on,
                on_page: None,
            }),
            request: ExportRequest {
                pages: PageRange::whole(page_count).expect("the fixture has pages"),
                dpi: 72.0,
                quality: None,
            },
            output_kind: kind,
            page_count,
        }
    }

    /// Text is one file and takes the name the user typed. So does a
    /// single-page PNG: numbering `report.png` to `report-01.png` when there
    /// is nothing to disambiguate it from would be surprising.
    #[test]
    fn a_one_file_export_keeps_the_name_the_user_chose() {
        let chosen = Path::new("/exports/report.png");

        assert_eq!(export_path(chosen, None, 1), PathBuf::from(chosen));
        assert_eq!(export_path(chosen, Some(0), 1), PathBuf::from(chosen));
    }

    /// A per-page export numbers beside the chosen name, one-based like the
    /// page controls and zero-padded so a directory listing sorts in page
    /// order rather than putting page 10 before page 2.
    #[test]
    fn a_per_page_export_numbers_one_based_beside_the_chosen_name() {
        let chosen = Path::new("/exports/report.png");

        assert_eq!(
            export_path(chosen, Some(0), 12),
            PathBuf::from("/exports/report-01.png")
        );
        assert_eq!(
            export_path(chosen, Some(9), 12),
            PathBuf::from("/exports/report-10.png")
        );
        assert_eq!(
            export_path(chosen, Some(0), 1_234),
            PathBuf::from("/exports/report-0001.png")
        );
        assert_eq!(
            export_path(chosen, Some(1_233), 1_234),
            PathBuf::from("/exports/report-1234.png")
        );
    }

    #[test]
    fn a_chosen_name_with_no_extension_still_numbers_its_pages() {
        assert_eq!(
            export_path(Path::new("/exports/report"), Some(1), 2),
            PathBuf::from("/exports/report-02")
        );
    }

    /// A name with dots in it keeps every one of them but the last: the stem
    /// of `q1.2026.png` is `q1.2026`, not `q1`.
    #[test]
    fn a_dotted_name_numbers_on_its_last_extension_only() {
        assert_eq!(
            export_path(Path::new("/exports/q1.2026.png"), Some(0), 2),
            PathBuf::from("/exports/q1.2026-01.png")
        );
    }

    #[test]
    fn the_export_phase_has_one_terminal_race_winner() {
        let cancellation_wins = ExportPhase::new();
        assert!(cancellation_wins.cancel());
        assert!(!cancellation_wins.begin_publishing());
        assert_eq!(cancellation_wins.load(), ExportPhaseValue::Cancelling);

        let publication_wins = ExportPhase::new();
        assert!(publication_wins.begin_publishing());
        assert!(!publication_wins.cancel());
        assert_eq!(publication_wins.load(), ExportPhaseValue::Publishing);
    }

    #[test]
    fn duplicate_per_page_destinations_fail_preflight() {
        let path = PathBuf::from("report-01.test");
        let failure = preflight_export_paths(&[path.clone(), path.clone()]).unwrap_err();

        assert!(
            matches!(failure, ExportFailure::Duplicate { path: duplicate } if duplicate == path)
        );
    }

    #[test]
    fn a_per_page_worker_streams_pages_in_absolute_order() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let phase = ExportPhase::new();
        let completed = AtomicUsize::new(0);
        let prepared =
            prepared_worker_export(ExportOutputKind::PerPage, Arc::clone(&calls), None, None, 3);

        assert_eq!(
            run_export_worker(prepared, &chosen, &phase, &completed).unwrap(),
            ExportOutcome::Complete
        );
        assert_eq!(*calls.lock().unwrap(), [0, 1, 2]);
        assert_eq!(completed.load(Ordering::Acquire), 3);
        assert_eq!(
            std::fs::read(dir.path().join("report-01.test")).unwrap(),
            b"page-0"
        );
        assert_eq!(
            std::fs::read(dir.path().join("report-02.test")).unwrap(),
            b"page-1"
        );
        assert_eq!(
            std::fs::read(dir.path().join("report-03.test")).unwrap(),
            b"page-2"
        );
    }

    #[test]
    fn immediate_prepublication_cancellation_wins_for_both_output_kinds() {
        for kind in [ExportOutputKind::Single, ExportOutputKind::PerPage] {
            let dir = tempfile::tempdir().expect("the test directory opens");
            let chosen = dir.path().join("report.test");
            let phase = Arc::new(ExportPhase::new());
            let cancel_phase = Arc::clone(&phase);
            let observer = RecordingExportObserver {
                before_publish: Some(Arc::new(move || {
                    assert!(cancel_phase.cancel());
                })),
                ..RecordingExportObserver::default()
            };
            let prepared =
                prepared_worker_export(kind, Arc::new(Mutex::new(Vec::new())), None, None, 2);

            assert_eq!(
                run_export_worker_observed(
                    prepared,
                    &chosen,
                    &phase,
                    &AtomicUsize::new(0),
                    &observer,
                )
                .unwrap(),
                ExportOutcome::Cancelled
            );
            assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
        }
    }

    #[test]
    fn a_blocked_single_file_worker_keeps_the_destination_absent_until_success() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let (started_tx, started_rx) = mpsc::channel();
        let document = Document::open_bytes(crate::shell::fixtures::many_pages_pdf(1)).unwrap();
        let prepared = PreparedExport {
            snapshot: document.export_snapshot().unwrap(),
            codec: Arc::new(BlockingCodec {
                kind: ExportOutputKind::Single,
                block_on: 0,
                calls,
                started: Mutex::new(Some(started_tx)),
                release: Arc::clone(&release),
            }),
            request: ExportRequest {
                pages: PageRange::whole(1).unwrap(),
                dpi: 72.0,
                quality: None,
            },
            output_kind: ExportOutputKind::Single,
            page_count: 1,
        };
        let worker_path = chosen.clone();
        let worker = std::thread::spawn(move || {
            run_export_worker(
                prepared,
                &worker_path,
                &ExportPhase::new(),
                &AtomicUsize::new(0),
            )
        });

        started_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(!chosen.exists());
        let (released, ready) = &*release;
        *released.lock().unwrap() = true;
        ready.notify_all();
        assert_eq!(worker.join().unwrap().unwrap(), ExportOutcome::Complete);
        assert_eq!(std::fs::read(chosen).unwrap(), b"page-0");
    }

    #[test]
    fn a_many_page_worker_never_has_more_than_one_destination_writer_open() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let observer = RecordingExportObserver::default();
        let prepared = prepared_worker_export(
            ExportOutputKind::PerPage,
            Arc::new(Mutex::new(Vec::new())),
            None,
            None,
            128,
        );

        assert_eq!(
            run_export_worker_observed(
                prepared,
                &chosen,
                &ExportPhase::new(),
                &AtomicUsize::new(0),
                &observer,
            )
            .unwrap(),
            ExportOutcome::Complete
        );
        assert_eq!(observer.open_writers.load(Ordering::Acquire), 0);
        assert_eq!(observer.max_open_writers.load(Ordering::Acquire), 1);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 128);
    }

    #[test]
    fn rollback_preserves_a_completed_page_replaced_by_another_writer() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let first = dir.path().join("report-01.test");
        let phase = Arc::new(ExportPhase::new());
        let replace_path = first.clone();
        let document = Document::open_bytes(crate::shell::fixtures::many_pages_pdf(3))
            .expect("the fixture opens");
        let prepared = PreparedExport {
            snapshot: document.export_snapshot().expect("the snapshot prepares"),
            codec: Arc::new(WorkerCodec {
                kind: ExportOutputKind::PerPage,
                calls: Arc::new(Mutex::new(Vec::new())),
                fail_on: None,
                cancel_on: Some((1, Arc::clone(&phase))),
                on_page: Some(Arc::new(move |page| {
                    if page == 1 {
                        std::fs::remove_file(&replace_path).unwrap();
                        std::fs::write(&replace_path, b"replacement").unwrap();
                    }
                })),
            }),
            request: ExportRequest {
                pages: PageRange::whole(3).unwrap(),
                dpi: 72.0,
                quality: None,
            },
            output_kind: ExportOutputKind::PerPage,
            page_count: 3,
        };

        assert_eq!(
            run_export_worker(prepared, &chosen, &phase, &AtomicUsize::new(0)).unwrap(),
            ExportOutcome::Cancelled
        );
        assert_eq!(std::fs::read(first).unwrap(), b"replacement");
        assert!(!dir.path().join("report-02.test").exists());
        assert!(!dir.path().join("report-03.test").exists());
    }

    #[cfg(unix)]
    #[test]
    fn rollback_preserves_a_completed_page_replaced_by_a_symlink() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let first = dir.path().join("report-01.test");
        let target = dir.path().join("target");
        std::fs::write(&target, b"target").unwrap();
        let phase = Arc::new(ExportPhase::new());
        let replace_path = first.clone();
        let link_target = target.clone();
        let document = Document::open_bytes(crate::shell::fixtures::many_pages_pdf(3)).unwrap();
        let prepared = PreparedExport {
            snapshot: document.export_snapshot().unwrap(),
            codec: Arc::new(WorkerCodec {
                kind: ExportOutputKind::PerPage,
                calls: Arc::new(Mutex::new(Vec::new())),
                fail_on: None,
                cancel_on: Some((1, Arc::clone(&phase))),
                on_page: Some(Arc::new(move |page| {
                    if page == 1 {
                        std::fs::remove_file(&replace_path).unwrap();
                        std::os::unix::fs::symlink(&link_target, &replace_path).unwrap();
                    }
                })),
            }),
            request: ExportRequest {
                pages: PageRange::whole(3).unwrap(),
                dpi: 72.0,
                quality: None,
            },
            output_kind: ExportOutputKind::PerPage,
            page_count: 3,
        };

        assert_eq!(
            run_export_worker(prepared, &chosen, &phase, &AtomicUsize::new(0)).unwrap(),
            ExportOutcome::Cancelled
        );
        assert!(std::fs::symlink_metadata(&first)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read(first).unwrap(), b"target");
    }

    #[test]
    fn rollback_never_deletes_a_completed_page_replaced_by_a_directory() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let first = dir.path().join("report-01.test");
        let phase = Arc::new(ExportPhase::new());
        let replace_path = first.clone();
        let document = Document::open_bytes(crate::shell::fixtures::many_pages_pdf(3)).unwrap();
        let prepared = PreparedExport {
            snapshot: document.export_snapshot().unwrap(),
            codec: Arc::new(WorkerCodec {
                kind: ExportOutputKind::PerPage,
                calls: Arc::new(Mutex::new(Vec::new())),
                fail_on: None,
                cancel_on: Some((1, Arc::clone(&phase))),
                on_page: Some(Arc::new(move |page| {
                    if page == 1 {
                        std::fs::remove_file(&replace_path).unwrap();
                        std::fs::create_dir(&replace_path).unwrap();
                        std::fs::write(replace_path.join("marker"), b"replacement").unwrap();
                    }
                })),
            }),
            request: ExportRequest {
                pages: PageRange::whole(3).unwrap(),
                dpi: 72.0,
                quality: None,
            },
            output_kind: ExportOutputKind::PerPage,
            page_count: 3,
        };

        let failure =
            run_export_worker(prepared, &chosen, &phase, &AtomicUsize::new(0)).unwrap_err();
        assert!(failure.to_string().contains("preserved in"), "{failure}");
        let preserved_marker = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path().join("0/marker"))
            .find(|path| path.exists())
            .expect("the substituted directory is retained in quarantine");
        assert_eq!(std::fs::read(preserved_marker).unwrap(), b"replacement");
    }

    #[test]
    fn an_existing_derived_file_fails_before_any_page_runs() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let conflict = dir.path().join("report-02.test");
        std::fs::write(&conflict, b"keep").unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let prepared =
            prepared_worker_export(ExportOutputKind::PerPage, Arc::clone(&calls), None, None, 3);

        let failure =
            run_export_worker(prepared, &chosen, &ExportPhase::new(), &AtomicUsize::new(0))
                .unwrap_err();

        assert!(matches!(failure, ExportFailure::Exists { path } if path == conflict));
        assert!(calls.lock().unwrap().is_empty());
        assert_eq!(std::fs::read(&conflict).unwrap(), b"keep");
        assert!(!dir.path().join("report-01.test").exists());
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_derived_symlink_fails_before_any_page_runs() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let conflict = dir.path().join("report-02.test");
        std::os::unix::fs::symlink(dir.path().join("missing"), &conflict).unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let prepared =
            prepared_worker_export(ExportOutputKind::PerPage, Arc::clone(&calls), None, None, 3);

        let failure =
            run_export_worker(prepared, &chosen, &ExportPhase::new(), &AtomicUsize::new(0))
                .unwrap_err();

        assert!(matches!(failure, ExportFailure::Exists { path } if path == conflict));
        assert!(calls.lock().unwrap().is_empty());
        assert!(std::fs::symlink_metadata(conflict)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[test]
    fn a_codec_failure_removes_every_per_page_output() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let prepared = prepared_worker_export(
            ExportOutputKind::PerPage,
            Arc::clone(&calls),
            Some(1),
            None,
            3,
        );

        let failure =
            run_export_worker(prepared, &chosen, &ExportPhase::new(), &AtomicUsize::new(0))
                .unwrap_err();

        assert!(failure.to_string().contains("encoding page 2"));
        assert_eq!(*calls.lock().unwrap(), [0, 1]);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn cancellation_after_page_one_removes_it_and_stops_before_page_three() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let calls = Arc::new(Mutex::new(Vec::new()));
        let phase = Arc::new(ExportPhase::new());
        let prepared = prepared_worker_export(
            ExportOutputKind::PerPage,
            Arc::clone(&calls),
            None,
            Some((1, Arc::clone(&phase))),
            3,
        );

        assert_eq!(
            run_export_worker(prepared, &chosen, &phase, &AtomicUsize::new(0)).unwrap(),
            ExportOutcome::Cancelled
        );
        assert_eq!(*calls.lock().unwrap(), [0, 1]);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn cancellation_after_the_only_codec_return_publishes_nothing() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let phase = Arc::new(ExportPhase::new());
        let prepared = prepared_worker_export(
            ExportOutputKind::Single,
            Arc::new(Mutex::new(Vec::new())),
            None,
            Some((0, Arc::clone(&phase))),
            1,
        );

        assert_eq!(
            run_export_worker(prepared, &chosen, &phase, &AtomicUsize::new(0)).unwrap(),
            ExportOutcome::Cancelled
        );
        assert!(!chosen.exists());
    }

    #[test]
    fn publication_preserves_an_existing_single_destination() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        std::fs::write(&chosen, b"keep").unwrap();
        let prepared = prepared_worker_export(
            ExportOutputKind::Single,
            Arc::new(Mutex::new(Vec::new())),
            None,
            None,
            2,
        );

        let failure =
            run_export_worker(prepared, &chosen, &ExportPhase::new(), &AtomicUsize::new(0))
                .unwrap_err();

        assert!(matches!(failure, ExportFailure::Exists { .. }));
        assert_eq!(std::fs::read(chosen).unwrap(), b"keep");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn a_late_cancel_cannot_override_single_file_publication() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let phase = Arc::new(ExportPhase::new());
        let cancel_phase = Arc::clone(&phase);
        let observer = RecordingExportObserver {
            after_publish_started: Some(Arc::new(move || {
                assert!(!cancel_phase.cancel());
            })),
            ..RecordingExportObserver::default()
        };
        let prepared = prepared_worker_export(
            ExportOutputKind::Single,
            Arc::new(Mutex::new(Vec::new())),
            None,
            None,
            2,
        );

        assert_eq!(
            run_export_worker_observed(prepared, &chosen, &phase, &AtomicUsize::new(0), &observer,)
                .unwrap(),
            ExportOutcome::Complete
        );
        assert_eq!(std::fs::read(chosen).unwrap(), b"page-0page-1");
    }

    #[test]
    fn cancellation_preserves_an_existing_single_destination() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        std::fs::write(&chosen, b"keep").unwrap();
        let phase = Arc::new(ExportPhase::new());
        let prepared = prepared_worker_export(
            ExportOutputKind::Single,
            Arc::new(Mutex::new(Vec::new())),
            None,
            Some((0, Arc::clone(&phase))),
            1,
        );

        assert_eq!(
            run_export_worker(prepared, &chosen, &phase, &AtomicUsize::new(0)).unwrap(),
            ExportOutcome::Cancelled
        );
        assert_eq!(std::fs::read(chosen).unwrap(), b"keep");
    }

    #[test]
    fn a_late_cancel_cannot_override_per_page_publication() {
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("report.test");
        let phase = Arc::new(ExportPhase::new());
        let cancel_phase = Arc::clone(&phase);
        let observer = RecordingExportObserver {
            after_publish_started: Some(Arc::new(move || {
                assert!(!cancel_phase.cancel());
            })),
            ..RecordingExportObserver::default()
        };
        let prepared = prepared_worker_export(
            ExportOutputKind::PerPage,
            Arc::new(Mutex::new(Vec::new())),
            None,
            None,
            2,
        );

        assert_eq!(
            run_export_worker_observed(prepared, &chosen, &phase, &AtomicUsize::new(0), &observer,)
                .unwrap(),
            ExportOutcome::Complete
        );
        assert_eq!(
            std::fs::read(dir.path().join("report-01.test")).unwrap(),
            b"page-0"
        );
        assert_eq!(
            std::fs::read(dir.path().join("report-02.test")).unwrap(),
            b"page-1"
        );
    }

    #[cfg(feature = "shell-test-support")]
    fn canvas_for_export(
        registry: PluginRegistry,
        cx: &mut TestAppContext,
    ) -> (
        Entity<Canvas>,
        onionskin_render::BaseRaster,
        &mut VisualTestContext,
    ) {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/hello.pdf");
        let mut document = Document::open_path(&path).expect("the seed opens");
        // The pixels the canvas would composite for this page at the export's
        // own resolution, taken before the document moves into the model so
        // the comparison is against the same worker and options.
        let on_screen = document
            .render_page_now(0, EXPORT_DPI / 72.0)
            .expect("the seed page renders")
            .raster;
        let model = CanvasModel::new(
            document,
            registry,
            ViewSize {
                width: 800.0,
                height: 600.0,
            },
        )
        .expect("the canvas model builds");
        let theme =
            ShellViewState::new(gpui::WindowAppearance::Dark, ThemePreference::System).tokens();
        let (canvas, cx) = cx.add_window_view(move |_window, _cx| Canvas::new(model, theme));
        (canvas, on_screen, cx)
    }

    #[cfg(feature = "shell-test-support")]
    fn export_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("onionskin-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("the test can make its own directory");
        dir
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn an_old_progress_timer_cannot_poll_a_rapidly_relaunched_export(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);

        window
            .update(cx, |frame, _window, cx| {
                let canvas = frame.tabs.tabs()[0].canvas.clone();
                let origin = canvas.entity_id();
                install_test_export_job(frame, origin);
                frame.finish_export(7, origin, &canvas, Ok(ExportOutcome::Complete), cx);
                let completed = Arc::new(AtomicUsize::new(2));
                frame.export.export_job = Some(ExportJob {
                    id: 8,
                    origin,
                    phase: Arc::new(ExportPhase::new()),
                    completed,
                    total: 3,
                    last_displayed: 0,
                });

                assert_eq!(frame.poll_export_progress(7), (false, false));
                assert_eq!(frame.export.export_job.as_ref().unwrap().last_displayed, 0);
                assert_eq!(frame.poll_export_progress(8), (true, true));
                assert_eq!(frame.export.export_job.as_ref().unwrap().last_displayed, 2);
            })
            .unwrap();
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn switching_tabs_does_not_cancel_the_origin_export(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf", "two-page.pdf"], cx);

        let phase = window
            .update(cx, |frame, _window, cx| {
                let origin = frame.tabs.tabs()[0].canvas.entity_id();
                let phase = install_test_export_job(frame, origin);
                frame.activate(1, cx);
                phase
            })
            .unwrap();

        assert_eq!(phase.load(), ExportPhaseValue::Running);
    }

    #[cfg(all(feature = "shell-test-support", feature = "codecs-common"))]
    #[gpui::test]
    fn a_second_export_is_refused_before_opening_another_prompt(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);

        window
            .update(cx, |frame, window, cx| {
                let origin = frame.tabs.tabs()[0].canvas.entity_id();
                install_test_export_job(frame, origin);
                frame.start_export(ExportTarget::Png, window, cx);
                frame.submit_export(window, cx);
            })
            .unwrap();

        assert!(!cx.did_prompt_for_new_path());
        window
            .update(cx, |frame, _window, cx| {
                assert!(matches!(
                    frame.tabs.tabs()[0].canvas.read(cx).model.status(),
                    Some(CanvasStatus::Error { message, .. })
                        if message.contains("already in progress")
                ));
            })
            .unwrap();
    }

    #[cfg(all(feature = "shell-test-support", feature = "codecs-common"))]
    #[gpui::test]
    fn two_pending_export_prompts_install_only_one_worker(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        let dir = tempfile::tempdir().expect("the test directory opens");
        let first = dir.path().join("first.png");
        let second = dir.path().join("second.png");

        window
            .update(cx, |frame, window, cx| {
                frame.start_export(ExportTarget::Png, window, cx);
                frame.submit_export(window, cx);
                frame.start_export(ExportTarget::Png, window, cx);
                frame.submit_export(window, cx);
            })
            .unwrap();
        cx.simulate_new_path_selection(|_| Some(first.clone()));
        cx.simulate_new_path_selection(|_| Some(second.clone()));
        cx.run_until_parked();

        assert!(first.exists());
        assert!(!second.exists());
        window
            .update(cx, |frame, _window, cx| {
                assert!(matches!(
                    frame.tabs.tabs()[0].canvas.read(cx).model.status(),
                    Some(CanvasStatus::Error { message, .. })
                        if message.contains("already in progress")
                ));
            })
            .unwrap();
    }

    #[cfg(all(feature = "shell-test-support", feature = "codecs-common"))]
    #[gpui::test]
    fn worker_failure_releases_progress_and_the_export_guard(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);
        let dir = tempfile::tempdir().expect("the test directory opens");
        let chosen = dir.path().join("failed.test");
        let completed = Arc::new(AtomicUsize::new(0));
        let prepared = prepared_worker_export(
            ExportOutputKind::PerPage,
            Arc::new(Mutex::new(Vec::new())),
            Some(1),
            None,
            3,
        );
        let result = run_export_worker(prepared, &chosen, &ExportPhase::new(), &completed);
        assert_eq!(completed.load(Ordering::Acquire), 1);
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());

        window
            .update(cx, |frame, window, cx| {
                let canvas = frame.tabs.tabs()[0].canvas.clone();
                let origin = canvas.entity_id();
                frame.export.export_job = Some(ExportJob {
                    id: 7,
                    origin,
                    phase: Arc::new(ExportPhase::new()),
                    completed: Arc::clone(&completed),
                    total: 3,
                    last_displayed: 0,
                });
                assert_eq!(frame.poll_export_progress(7), (true, true));
                frame.start_export(ExportTarget::Png, window, cx);
                frame.submit_export(window, cx);
                assert!(frame.export.export_job.is_some());
                frame.finish_export(7, origin, &canvas, result, cx);
                assert!(frame.export.export_job.is_none());
                assert!(frame
                    .accessible(window, cx)
                    .find(&"export-progress".into())
                    .is_none());
                frame.start_export(ExportTarget::Png, window, cx);
                frame.submit_export(window, cx);
            })
            .unwrap();

        assert!(cx.did_prompt_for_new_path());
        cx.simulate_new_path_selection(|_| None);
        cx.run_until_parked();
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn closing_the_origin_tab_cancels_its_export(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf", "two-page.pdf"], cx);

        let phase = window
            .update(cx, |frame, _window, cx| {
                let origin = frame.tabs.tabs()[0].canvas.entity_id();
                let phase = install_test_export_job(frame, origin);
                frame.run_tab_command(TabCommand::Close, 0, cx).unwrap();
                phase
            })
            .unwrap();

        assert_eq!(phase.load(), ExportPhaseValue::Cancelling);
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn close_others_and_close_all_cancel_a_removed_origin(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf", "two-page.pdf"], cx);
        let close_others_phase = window
            .update(cx, |frame, _window, cx| {
                let origin = frame.tabs.tabs()[0].canvas.entity_id();
                let phase = install_test_export_job(frame, origin);
                frame
                    .run_tab_command(TabCommand::CloseOthers, 1, cx)
                    .unwrap();
                phase
            })
            .unwrap();
        assert_eq!(close_others_phase.load(), ExportPhaseValue::Cancelling);

        let origin = window
            .update(cx, |frame, _window, _cx| {
                frame.tabs.tabs()[0].canvas.entity_id()
            })
            .unwrap();
        let close_all_phase = window
            .update(cx, |frame, _window, cx| {
                let phase = install_test_export_job(frame, origin);
                frame.run_tab_command(TabCommand::CloseAll, 0, cx).unwrap();
                phase
            })
            .unwrap();
        assert_eq!(close_all_phase.load(), ExportPhaseValue::Cancelling);
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn export_progress_and_cancellation_are_accessible(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf"], cx);

        let phase = window
            .update(cx, |frame, window, cx| {
                let origin = frame.tabs.tabs()[0].canvas.entity_id();
                let phase = install_test_export_job(frame, origin);
                let tree = frame.accessible(window, cx);
                assert_eq!(
                    tree.find(&"export-progress".into()).unwrap().label,
                    "Exporting 1 of 3 pages"
                );
                let cancel = tree.find(&"cancel-export".into()).unwrap();
                assert_eq!(cancel.activation, Some(Activation::CancelExport));
                assert!(!cancel.state.disabled);
                frame.run_activation(Activation::CancelExport, window, cx);
                phase
            })
            .unwrap();

        assert_eq!(phase.load(), ExportPhaseValue::Cancelling);
        window
            .update(cx, |frame, window, cx| {
                let tree = frame.accessible(window, cx);
                assert_eq!(
                    tree.find(&"export-progress".into()).unwrap().label,
                    "Cancelling export, 1 of 3 pages complete"
                );
                assert!(tree.find(&"cancel-export".into()).unwrap().state.disabled);
            })
            .unwrap();
    }

    #[cfg(all(feature = "shell-test-support", feature = "codecs-common"))]
    #[gpui::test]
    fn stale_export_prompt_after_switching_tabs_writes_nothing(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf", "two-page.pdf"], cx);
        let dir = export_dir("stale-export-switch");
        let chosen = dir.join("hello.png");

        window
            .update(cx, |frame, window, cx| {
                frame.start_export(ExportTarget::Png, window, cx);
                frame.submit_export(window, cx);
            })
            .unwrap();
        assert!(cx.did_prompt_for_new_path());

        window
            .update(cx, |frame, _window, cx| {
                frame.activate(1, cx);
            })
            .unwrap();
        cx.simulate_new_path_selection(|_| Some(chosen.clone()));
        cx.run_until_parked();

        assert!(!chosen.exists(), "a stale export wrote after tab switch");
        std::fs::remove_dir_all(&dir).expect("the test cleans up after itself");
    }

    #[cfg(all(feature = "shell-test-support", feature = "codecs-common"))]
    #[gpui::test]
    fn stale_export_prompt_after_closing_the_tab_writes_nothing(cx: &mut TestAppContext) {
        let (window, _) = bound_window(&["hello.pdf", "two-page.pdf"], cx);
        let dir = export_dir("stale-export-close");
        let chosen = dir.join("hello.png");

        window
            .update(cx, |frame, window, cx| {
                frame.start_export(ExportTarget::Png, window, cx);
                frame.submit_export(window, cx);
            })
            .unwrap();
        assert!(cx.did_prompt_for_new_path());

        window
            .update(cx, |frame, _window, cx| {
                frame.run_tab_command(TabCommand::Close, 0, cx).unwrap();
            })
            .unwrap();
        cx.simulate_new_path_selection(|_| Some(chosen.clone()));
        cx.run_until_parked();

        assert!(!chosen.exists(), "a stale export wrote after tab close");
        std::fs::remove_dir_all(&dir).expect("the test cleans up after itself");
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn stale_attachment_prompt_after_switching_tabs_writes_nothing(cx: &mut TestAppContext) {
        let (window, _) = bound_window_from_bytes(
            vec![
                (
                    "with-attachment.pdf",
                    crate::shell::fixtures::attachment_pdf(),
                ),
                ("other.pdf", crate::shell::fixtures::outline_pdf()),
            ],
            cx,
        );
        let dir = export_dir("stale-attachment-switch");
        let chosen = dir.join("notes.txt");

        window
            .update(cx, |frame, _window, cx| {
                frame.run_pane_action(PaneAction::Select(NavigationPane::Attachments), cx);
                frame.run_pane_action(PaneAction::Attachment(panes::AttachmentAction::Save(0)), cx);
            })
            .unwrap();
        assert!(cx.did_prompt_for_new_path());

        window
            .update(cx, |frame, _window, cx| {
                frame.activate(1, cx);
            })
            .unwrap();
        cx.simulate_new_path_selection(|_| Some(chosen.clone()));
        cx.run_until_parked();

        assert!(
            !chosen.exists(),
            "a stale attachment save wrote after tab switch"
        );
        std::fs::remove_dir_all(&dir).expect("the test cleans up after itself");
    }

    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn stale_attachment_prompt_after_closing_the_tab_writes_nothing(cx: &mut TestAppContext) {
        let (window, _) = bound_window_from_bytes(
            vec![
                (
                    "with-attachment.pdf",
                    crate::shell::fixtures::attachment_pdf(),
                ),
                ("other.pdf", crate::shell::fixtures::outline_pdf()),
            ],
            cx,
        );
        let dir = export_dir("stale-attachment-close");
        let chosen = dir.join("notes.txt");

        window
            .update(cx, |frame, _window, cx| {
                frame.run_pane_action(PaneAction::Select(NavigationPane::Attachments), cx);
                frame.run_pane_action(PaneAction::Attachment(panes::AttachmentAction::Save(0)), cx);
            })
            .unwrap();
        assert!(cx.did_prompt_for_new_path());

        window
            .update(cx, |frame, _window, cx| {
                frame.run_tab_command(TabCommand::Close, 0, cx).unwrap();
            })
            .unwrap();
        cx.simulate_new_path_selection(|_| Some(chosen.clone()));
        cx.run_until_parked();

        assert!(
            !chosen.exists(),
            "a stale attachment save wrote after tab close"
        );
        std::fs::remove_dir_all(&dir).expect("the test cleans up after itself");
    }

    /// The whole menu path bar the file dialog, which cannot be driven
    /// headless: the codec id `MenuCommand::Export(Png)` carries, looked up in
    /// the registry the canvas holds, exported and written. The bytes on disk
    /// are the pixels the canvas composites, which is the point of routing
    /// export through `core` rather than beside it.
    #[cfg(all(feature = "shell-test-support", feature = "codecs-common"))]
    #[gpui::test]
    fn the_png_menu_entry_writes_the_canvas_paths_own_pixels(cx: &mut TestAppContext) {
        let mut registry = PluginRegistry::new();
        registry.install(&onionskin_codecs_common::CommonCodecsPlugin);
        let (canvas, on_screen, cx) = canvas_for_export(registry, cx);
        let dir = export_dir("png-export");
        let chosen = dir.join("hello.png");

        cx.update(|_window, app| run_export(&canvas, ExportTarget::Png, &chosen, app));

        let decoded = image::load_from_memory_with_format(
            &std::fs::read(&chosen).expect("the export reached disk"),
            image::ImageFormat::Png,
        )
        .expect("the export is a PNG")
        .to_rgba8();
        assert_eq!(
            decoded.dimensions(),
            (on_screen.width(), on_screen.height())
        );
        assert_eq!(decoded.into_raw(), on_screen.rgba());
        cx.update(|_window, app| {
            let status = canvas.read(app).model.status();
            assert!(
                !matches!(status, Some(CanvasStatus::Error { .. })),
                "{status:?}"
            );
        });
        std::fs::remove_dir_all(&dir).expect("the test cleans up after itself");
    }

    /// Text is one file for the whole document, so it takes the chosen name
    /// unnumbered.
    #[cfg(all(feature = "shell-test-support", feature = "codecs-common"))]
    #[gpui::test]
    fn the_text_menu_entry_writes_one_file_at_the_chosen_name(cx: &mut TestAppContext) {
        let mut registry = PluginRegistry::new();
        registry.install(&onionskin_codecs_common::CommonCodecsPlugin);
        let (canvas, _, cx) = canvas_for_export(registry, cx);
        let dir = export_dir("text-export");
        let chosen = dir.join("hello.txt");

        cx.update(|_window, app| run_export(&canvas, ExportTarget::Text, &chosen, app));

        assert!(std::fs::read_to_string(&chosen)
            .expect("the export reached disk")
            .contains("Hello"));
        assert_eq!(
            std::fs::read_dir(&dir).unwrap().count(),
            1,
            "a whole-document text export is one file"
        );
        std::fs::remove_dir_all(&dir).expect("the test cleans up after itself");
    }

    /// A destination that cannot be written surfaces on the document the
    /// export came from, naming the file, rather than only on stderr.
    #[cfg(all(feature = "shell-test-support", feature = "codecs-common"))]
    #[gpui::test]
    fn an_unwritable_destination_is_reported_on_the_document(cx: &mut TestAppContext) {
        let mut registry = PluginRegistry::new();
        registry.install(&onionskin_codecs_common::CommonCodecsPlugin);
        let (canvas, _, cx) = canvas_for_export(registry, cx);
        let chosen = Path::new("/onionskin-does-not-exist/hello.txt");

        cx.update(|_window, app| run_export(&canvas, ExportTarget::Text, chosen, app));

        cx.update(|_window, app| {
            let Some(CanvasStatus::Error { page, message }) = canvas.read(app).model.status()
            else {
                panic!("the failure did not reach the document");
            };
            assert_eq!(*page, None);
            assert!(
                message.starts_with("/onionskin-does-not-exist/hello.txt could not be written: "),
                "{message}"
            );
        });
    }

    /// With the plugin compiled out the menu entry is disabled, but the run
    /// path still refuses by name rather than writing an empty file.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn exporting_without_the_codec_installed_says_which_one_is_missing(cx: &mut TestAppContext) {
        let (canvas, _, cx) = canvas_for_export(PluginRegistry::new(), cx);
        let dir = export_dir("absent-codec");
        let chosen = dir.join("hello.png");

        cx.update(|_window, app| run_export(&canvas, ExportTarget::Png, &chosen, app));

        assert!(!chosen.exists(), "nothing should have been written");
        cx.update(|_window, app| {
            let Some(CanvasStatus::Error { message, .. }) = canvas.read(app).model.status() else {
                panic!("the failure did not reach the document");
            };
            assert_eq!(message, "export failed: no png codec is installed");
        });
        std::fs::remove_dir_all(&dir).expect("the test cleans up after itself");
    }
}
