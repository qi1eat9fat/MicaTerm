// 来自 since2006/shell-rs @ b2a307a: src/sftp/describe.rs
// 裁剪：去掉英文、复数写法（`tn!`）、CLI 同步和单栏「文件」的错误。
//! The SFTP engine's errors, questions and notes in the user's words. The
//! engine only says what happened; this is where it is worded, at the
//! moment it is shown.
//!
//! An error is worded link by link down its chain, the way `{:#}` prints
//! it: the links the engine knows here, the SSH layer's by its own
//! wording, and any other as it says itself. A link that says what lies
//! under it in a sentence of its own, such as 「无法上传：…」, ends the
//! chain there.

use std::error::Error;

use super::{
    Cancelled, Disconnect, DownloadError, EditError, JournalError, ReadFailure, Reason,
    SaveFailure, SftpError, UploadError,
};
use crate::backend::describe::{describe_login_error, link as describe_link};
use crate::i18n::t;
use micaterm_ssh::link_as;

type Cause<'a> = &'a (dyn Error + 'static);

/// The error's own words, without what lies under it: what `to_string()`
/// gives, worded.
pub fn describe_sftp_error(error: &anyhow::Error) -> String {
    let causes: Vec<Cause<'_>> = error.chain().collect();
    link(causes[0], &causes[1..])
}

/// The error down to its cause, the way `{:#}` prints it, worded.
pub fn describe_sftp_chain(error: &anyhow::Error) -> String {
    let causes: Vec<Cause<'_>> = error.chain().collect();
    chain_text(&causes)
}

/// A reason down to its cause: why a request or an item failed.
pub fn describe_reason(reason: &Reason) -> String {
    describe_sftp_chain(reason.error())
}

/// A reason's first link: a question, a note, a notice. A failure's first
/// link says what lies under it itself.
pub fn describe_reason_link(reason: &Reason) -> String {
    describe_sftp_error(reason.error())
}

/// Why there is no connection now.
pub fn describe_disconnect(why: &Disconnect) -> String {
    match why {
        Disconnect::ByUser => t!("sftp.worker.disconnected").into(),
        Disconnect::Dropped => t!("sftp.worker.dropped").into(),
        Disconnect::DroppedKept => t!("sftp.worker.dropped_kept").into(),
        Disconnect::LoginFailed(reason) => {
            let error = reason.error();
            // Starting SFTP failed after the login, or never got to it:
            // the login's own wording knows none of that.
            if error.chain().any(|cause| words(cause, &[]).is_some()) {
                describe_sftp_chain(error)
            } else {
                describe_login_error(error)
            }
        }
        Disconnect::Failed(reason) => describe_reason_link(reason),
    }
}

/// Why a file was not opened for editing, in a sentence.
pub fn describe_read_failure(failure: &ReadFailure) -> String {
    match failure {
        ReadFailure::NotFile => t!("sftp.edit.not_file").into(),
        ReadFailure::TooLarge(size) => t!("sftp.edit.too_large", count = size).into(),
        ReadFailure::NotText => t!("sftp.edit.not_text").into(),
        ReadFailure::Failed(message) => message.clone(),
    }
}

/// Why a save did not happen or did not finish, in a sentence.
pub fn describe_save_failure(failure: &SaveFailure) -> String {
    match failure {
        SaveFailure::Changed { .. } => t!("sftp.edit.changed").into(),
        SaveFailure::Interrupted(message) | SaveFailure::Failed(message) => message.clone(),
    }
}

/// Every link of `causes` in words, joined as `{:#}` joins them, up to one
/// that words the rest itself.
fn chain_text(causes: &[Cause<'_>]) -> String {
    let mut text = Vec::new();
    for (ix, &cause) in causes.iter().enumerate() {
        text.push(link(cause, &causes[ix + 1..]));
        if says_the_rest(cause) {
            break;
        }
    }
    text.join(": ")
}

/// Whether `cause` puts what lies under it in a sentence of its own.
fn says_the_rest(cause: Cause<'_>) -> bool {
    matches!(
        link_as::<SftpError>(cause),
        Some(SftpError::ListFailed | SftpError::DeleteFailed)
    ) || matches!(link_as::<UploadError>(cause), Some(UploadError::Failed))
        || matches!(link_as::<DownloadError>(cause), Some(DownloadError::Failed))
        || matches!(link_as::<EditError>(cause), Some(EditError::Interrupted))
}

/// One link of a chain in words. `rest` is what lies under it.
fn link(cause: Cause<'_>, rest: &[Cause<'_>]) -> String {
    words(cause, rest).unwrap_or_else(|| describe_link(cause, rest))
}

/// The words for a link the SFTP engine made, and `None` for any other.
fn words(cause: Cause<'_>, rest: &[Cause<'_>]) -> Option<String> {
    if let Some(error) = link_as::<SftpError>(cause) {
        return Some(sftp(error, rest));
    }
    if let Some(error) = link_as::<UploadError>(cause) {
        return Some(upload(error, rest));
    }
    if let Some(error) = link_as::<DownloadError>(cause) {
        return Some(download(error, rest));
    }
    if let Some(error) = link_as::<JournalError>(cause) {
        return Some(journal(error));
    }
    if let Some(error) = link_as::<EditError>(cause) {
        return Some(edit(error, rest));
    }
    if cause.is::<Cancelled>() {
        return Some(t!("sftp.transfer.cancelled").into());
    }
    // An I/O error the engine put one of its own in.
    let io = cause.downcast_ref::<std::io::Error>()?;
    words(io.get_ref()?, &[])
}

fn sftp(error: &SftpError, rest: &[Cause<'_>]) -> String {
    match error {
        SftpError::StartTimeout => t!("sftp.client.start_timeout"),
        SftpError::NoV3 => t!("sftp.client.no_v3"),
        SftpError::NoPath => t!("sftp.client.no_path"),
        SftpError::NoLinkTarget => t!("sftp.client.no_link_target"),
        SftpError::AmbiguousName => t!("sftp.client.ambiguous_name"),
        SftpError::RemotePathInvalid => t!("sftp.error.remote_path_invalid"),
        SftpError::FileNameInvalid { name } => t!("sftp.error.file_name_invalid", name = name),
        SftpError::NothingToUpload => t!("sftp.error.nothing_to_upload"),
        SftpError::NothingToDownload => t!("sftp.error.nothing_to_download"),
        SftpError::DownloadTargetRelative => t!("sftp.error.download_target_relative"),
        SftpError::FileNameNotUtf8 => t!("sftp.error.file_name_not_utf8"),
        SftpError::Exists { name } => t!("sftp.error.exists", name = name),
        SftpError::HostKeyChanged => t!("sftp.transfer.host_key_changed"),
        SftpError::NotConnected => t!("sftp.worker.not_connected"),
        SftpError::NoConnection => t!("sftp.worker.not_connected_short"),
        SftpError::ListFailed => t!("sftp.worker.list_failed", error = chain_text(rest)),
        SftpError::DeleteFailed => t!("sftp.sync.delete_failed", error = chain_text(rest)),
        SftpError::Busy => t!("sftp.worker.busy"),
        SftpError::DiscardOffline => t!("sftp.worker.discard_offline"),
        SftpError::LocalFolderMissing { path } => {
            t!("sftp.worker.local_folder_missing", path = path.display())
        }
        SftpError::RemoteFolderMissing { path } => {
            t!("sftp.worker.remote_folder_missing", path = path)
        }
        SftpError::NotAFolder { path } => t!("sftp.worker.not_a_folder", path = path),
        SftpError::SyncToRoot => t!("sftp.worker.sync_to_root"),
        SftpError::ReconnectIn { seconds, attempt } => {
            t!(
                "sftp.worker.reconnect_in",
                count = seconds,
                attempt = attempt
            )
        }
    }
    .into()
}

fn upload(error: &UploadError, rest: &[Cause<'_>]) -> String {
    match error {
        UploadError::SourceNameNotUtf8 => t!("sftp.upload.source_name_not_utf8"),
        UploadError::TargetBusy => t!("sftp.upload.target_busy"),
        UploadError::ParentMissing => t!("sftp.upload.parent_missing"),
        UploadError::Failed => t!("sftp.upload.failed", error = chain_text(rest)),
        UploadError::FolderInTheWay => t!("sftp.upload.folder_in_the_way"),
        UploadError::Exists { size, modified } => {
            t!(
                "sftp.upload.exists",
                count = size,
                modified = format!("{modified:?}")
            )
        }
        UploadError::ExistsChanged { size, modified } => t!(
            "sftp.upload.exists_changed",
            count = size,
            modified = format!("{modified:?}")
        ),
        UploadError::TargetGone => t!("sftp.upload.target_gone"),
        UploadError::SourceNowFolder => t!("sftp.upload.source_now_folder"),
        UploadError::TargetNotFolder => t!("sftp.upload.target_not_folder"),
        UploadError::SpecialFile => t!("sftp.upload.special_file"),
        UploadError::ResumeFound => t!("sftp.upload.resume_found"),
        UploadError::LinkTargetNotUtf8 => t!("sftp.upload.link_target_not_utf8"),
        UploadError::RecordWithoutLink => t!("sftp.upload.record_without_link"),
        UploadError::TemporaryLinkChanged => t!("sftp.upload.temporary_link_changed"),
        UploadError::RecordWithoutSource => t!("sftp.upload.record_without_source"),
        UploadError::TemporaryKindChanged => t!("sftp.upload.temporary_kind_changed"),
        UploadError::PartialTooLarge => t!("sftp.upload.partial_too_large"),
        UploadError::PipelineEnded => t!("sftp.upload.pipeline_ended"),
        UploadError::PublishUnconfirmed => t!("sftp.upload.publish_unconfirmed"),
        UploadError::TemporaryChanged => t!("sftp.upload.temporary_changed"),
        UploadError::TargetCreated => t!("sftp.upload.target_created"),
        UploadError::BackupChanged => t!("sftp.upload.backup_changed"),
        UploadError::BackupChangedAfterPublish => t!("sftp.upload.backup_changed_after_publish"),
        UploadError::BackupAndTarget => t!("sftp.upload.backup_and_target"),
    }
    .into()
}

fn download(error: &DownloadError, rest: &[Cause<'_>]) -> String {
    match error {
        DownloadError::NameInvalid { name } => t!("sftp.download.name_invalid", name = name),
        DownloadError::SourceMissing => t!("sftp.download.source_missing"),
        DownloadError::TargetBusy => t!("sftp.download.target_busy"),
        DownloadError::ParentMissing => t!("sftp.download.parent_missing"),
        DownloadError::Failed => t!("sftp.download.failed", error = chain_text(rest)),
        DownloadError::TargetInvalid => t!("sftp.download.target_invalid"),
        DownloadError::FolderInTheWay => t!("sftp.download.folder_in_the_way"),
        DownloadError::Exists { size, modified } => t!(
            "sftp.download.exists",
            count = size,
            modified = format!("{modified:?}")
        ),
        DownloadError::ExistsChanged { size, modified } => t!(
            "sftp.download.exists_changed",
            count = size,
            modified = format!("{modified:?}")
        ),
        DownloadError::FileInTheWay => t!("sftp.download.file_in_the_way"),
        DownloadError::SpecialFile => t!("sftp.download.special_file"),
        DownloadError::PartialNotFile { path } => {
            t!("sftp.download.partial_not_file", path = path.display())
        }
        DownloadError::PartialTooLarge => t!("sftp.download.partial_too_large"),
        DownloadError::SourceChanged => t!("sftp.download.source_changed"),
        DownloadError::ResumeFound => t!("sftp.download.resume_found"),
        DownloadError::PipelineEnded => t!("sftp.download.pipeline_ended"),
        DownloadError::SourceShrank => t!("sftp.download.source_shrank"),
        DownloadError::LinksUnsupported => t!("sftp.download.links_unsupported"),
    }
    .into()
}

fn journal(error: &JournalError) -> String {
    match error {
        JournalError::SourceNotFile => t!("sftp.journal.source_not_file"),
        JournalError::Mismatch => t!("sftp.journal.mismatch"),
        JournalError::TemporaryInvalid => t!("sftp.journal.temporary_invalid"),
        JournalError::ReplacesFolder => t!("sftp.journal.replaces_folder"),
        JournalError::UploadDamaged => t!("sftp.journal.upload_damaged"),
        JournalError::DownloadDamaged => t!("sftp.journal.download_damaged"),
        JournalError::Unreadable => t!("sftp.journal.unreadable"),
        JournalError::Unsaved => t!("sftp.journal.unsaved"),
    }
    .into()
}

fn edit(error: &EditError, rest: &[Cause<'_>]) -> String {
    match error {
        EditError::Missing => t!("sftp.edit.missing").into(),
        EditError::Shrank => t!("sftp.edit.shrank").into(),
        EditError::GoneAfterSave => t!("sftp.edit.gone_after_save").into(),
        // Said by what cut the write short.
        EditError::Interrupted => chain_text(rest),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;
    use micaterm_ssh::{ConnectError, wrap};
    use std::io;

    fn reason(error: impl Error + Send + Sync + 'static) -> Reason {
        Reason::new(error)
    }

    #[test]
    fn engine_errors_read_as_they_did() {
        assert_eq!(
            describe_reason(&reason(SftpError::Exists { name: "b".into() })),
            "An item named \"b\" already exists"
        );
        assert_eq!(
            describe_reason(&reason(SftpError::LocalFolderMissing {
                path: "/tmp/没有".into()
            })),
            "The local folder /tmp/没有 does not exist"
        );
        assert_eq!(
            describe_reason(&reason(DownloadError::NameInvalid { name: "a:b".into() })),
            "The name \"a:b\" is not valid on this computer"
        );
        assert_eq!(
            describe_reason(&reason(JournalError::Mismatch)),
            "The resume record does not match the current source or server"
        );
        assert_eq!(
            describe_reason(&reason(EditError::Shrank)),
            "The file became shorter while reading; open it again"
        );
        assert_eq!(
            describe_reason(&reason(Cancelled)),
            "The transfer stopped; progress was kept"
        );
        // The modification time as it always read: debug-formatted seconds.
        assert_eq!(
            describe_reason(&reason(UploadError::Exists {
                size: 12,
                modified: Some(1_700_000_000)
            })),
            "The remote item already exists (12 bytes, modified Some(1700000000)). Overwriting replaces this file or link."
        );
        assert_eq!(
            describe_reason(&reason(DownloadError::ExistsChanged {
                size: 3,
                modified: None
            })),
            "The target changed during the download. An item with the same name already exists locally (3 bytes, modified None). Overwriting replaces it."
        );
    }

    #[test]
    fn a_layer_says_its_cause_in_its_own_sentence() {
        let cause = Reason::from(wrap(
            JournalError::Unreadable,
            io::Error::new(io::ErrorKind::PermissionDenied, "denied"),
        ));
        assert_eq!(
            describe_reason(&cause),
            "Could not read the resume record: denied"
        );
        let failed = cause.under(UploadError::Failed);
        // The question shows the first link, which says it all.
        assert_eq!(
            describe_reason_link(&failed),
            "Cannot upload: Could not read the resume record: denied"
        );
        assert_eq!(describe_reason(&failed), describe_reason_link(&failed));
        let listing = Reason::from(anyhow!("no such file")).under(SftpError::ListFailed);
        assert_eq!(
            describe_reason(&listing),
            "Cannot read this directory: no such file"
        );
        let deleting = Reason::from(anyhow!("busy")).under(SftpError::DeleteFailed);
        assert_eq!(describe_reason_link(&deleting), "Could not delete: busy");
        let downloading = Reason::new(DownloadError::SourceShrank).under(DownloadError::Failed);
        assert_eq!(
            describe_reason_link(&downloading),
            "Cannot download: The remote file became shorter during the download; stopped"
        );
        // A notice shows only the first link; a detail all of it.
        let unsaved = Reason::from(wrap(JournalError::Unsaved, anyhow!("disk full")));
        assert_eq!(describe_reason_link(&unsaved), "Could not save resume progress");
        assert_eq!(
            describe_reason(&unsaved),
            "Could not save resume progress: disk full"
        );
    }

    #[test]
    fn an_engine_error_inside_an_io_error_is_worded_too() {
        let error = anyhow::Error::from(io::Error::new(
            io::ErrorKind::NotConnected,
            SftpError::NoConnection,
        ));
        assert_eq!(describe_sftp_error(&error), "SFTP is not connected");
    }

    #[test]
    fn why_there_is_no_connection() {
        assert_eq!(
            describe_disconnect(&Disconnect::ByUser),
            "SFTP is disconnected"
        );
        assert_eq!(
            describe_disconnect(&Disconnect::Dropped),
            "The SFTP connection dropped; reconnect to continue"
        );
        assert_eq!(
            describe_disconnect(&Disconnect::DroppedKept),
            "The connection dropped; transfer progress was kept"
        );
        // Starting SFTP is the engine's to word, logging in the SSH layer's.
        let slow = Disconnect::LoginFailed(reason(SftpError::StartTimeout));
        assert_eq!(describe_disconnect(&slow), "Starting SFTP timed out");
        let login = anyhow!(ConnectError::Handshake);
        let expected = describe_login_error(&login);
        assert_eq!(
            describe_disconnect(&Disconnect::LoginFailed(login.into())),
            expected
        );
        // A failed reconnect says what failed, not what lies under it.
        let failed = wrap(ConnectError::Handshake, anyhow!("eof"));
        let expected = describe_link(failed.chain().next().unwrap(), &[]);
        assert_eq!(
            describe_disconnect(&Disconnect::Failed(failed.into())),
            expected
        );
    }

    #[test]
    fn the_editor_s_failures_read_as_they_did() {
        assert_eq!(
            describe_read_failure(&ReadFailure::NotFile),
            "Not a regular file"
        );
        assert_eq!(
            describe_read_failure(&ReadFailure::TooLarge(6_000_000)),
            "The file is too large (6000000 bytes)"
        );
        assert_eq!(
            describe_read_failure(&ReadFailure::Failed("文件不存在".into())),
            "文件不存在"
        );
        assert_eq!(
            describe_save_failure(&SaveFailure::Changed { now: None }),
            "The file was changed after it was opened"
        );
        assert_eq!(
            describe_save_failure(&SaveFailure::Interrupted("reset".into())),
            "reset"
        );
    }
}
