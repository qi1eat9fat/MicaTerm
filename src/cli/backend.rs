//! 真的后端：`exec`（Agent CLI 工单 02）经 SSH 核心登录、跑命令；`upload` / `download` / `sync`
//! （工单 03）走和 SFTP 标签同一个传输引擎。都没人可问：要问的问题当场拒绝，说清是哪一种。
//! 每个请求一条自己的连接，不进连接表、不进 SFTP 标签的队列：不和卡片共用，做完就断。
// 参考 since2006/shell-rs @ b2a307a: src/cli/backend.rs（去掉堡垒机外部连接的提示，加上读秘密时
// 才发现保险库锁着的情形；传输停下（重连三次都没连上）也算结束）

use std::{
    io,
    path::Path,
    time::{Duration, Instant},
};

use micaterm_hosts::secrets::SecretsLocked;
use micaterm_ssh::{
    ConnectionPrompt, ConnectionPromptKind, ConnectionPromptReply, ExecErrorKind, ExecExit,
    ExecStream, SshConnectionConfig, SshConnector,
};

use super::hosts::CliTarget;
use super::protocol::{CliError, ErrorCode, TransferCounters, TransferSummary};
use super::server::{CliBackend, vault_locked};
use crate::backend::describe_exec_error;
use crate::backend::sftp::{
    DownloadRequest, RemotePath, SftpCommand, SftpEvent, SharedSftpTransportProvider,
    TransferAnswer, TransferChoice, TransferPhase, TransferProgress, TransferQuestionKind,
    UploadRequest, describe_reason_link, describe_sftp_error,
};
use crate::i18n::t;

/// 传输多久向命令报一次进度。
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);

pub struct SshCliBackend {
    connector: SshConnector,
    sftp: SharedSftpTransportProvider,
}

impl SshCliBackend {
    /// 和远程卡片用同一份 known_hosts、同一个保险库：主机密钥只信任一处；传输和 SFTP 标签用同一个
    /// 引擎（续传记录也在同一处）。
    pub fn new(connector: SshConnector, sftp: SharedSftpTransportProvider) -> Self {
        Self { connector, sftp }
    }

    /// 在一条自己的连接上做一次传输，每个问题都照没人看着的拷贝那样答。
    fn transfer(
        &self,
        target: &CliTarget,
        command: SftpCommand,
        progress: &mut dyn FnMut(TransferCounters) -> io::Result<()>,
    ) -> Result<TransferSummary, CliError> {
        let transport = self.sftp.create(target.login());
        let (commands, command_receiver) = async_channel::unbounded();
        let (event_sender, events) = async_channel::unbounded();
        let worker = std::thread::Builder::new()
            .name("micaterm-cli-sftp".into())
            .spawn(move || transport.run(command_receiver, event_sender))
            .map_err(|error| CliError::new(ErrorCode::ConnectFailed, error.to_string()))?;
        let mut run = Transfer::default();

        let result = (|| {
            // 先连上：登录走不下去就照实说，不重试。
            loop {
                match events.recv_blocking() {
                    Ok(SftpEvent::Connected { .. }) => break,
                    Ok(SftpEvent::Prompt(prompt)) => run.refuse(&commands, &prompt),
                    Ok(SftpEvent::Disconnected(reason)) => return Err(run.failure(reason)),
                    Ok(_) => {}
                    Err(_) => return Err(run.failure(t!("cli.transfer.sftp_ended").into())),
                }
            }
            let _ = commands.send_blocking(command);
            let mut reported = Instant::now() - PROGRESS_INTERVAL;
            loop {
                match events.recv_blocking() {
                    Ok(SftpEvent::Progress(state)) => {
                        let due = reported.elapsed() >= PROGRESS_INTERVAL;
                        if due && !run.abandoned {
                            reported = Instant::now();
                            if progress(counters(&state)).is_err() {
                                run.abandoned = true;
                                let _ = commands.send_blocking(SftpCommand::Cancel);
                            }
                        }
                        // 重连三次都没连上，这一批停下等人：命令这边算结束。
                        let stopped = state.phase() == TransferPhase::Stopped;
                        run.last = Some(state);
                        if stopped {
                            break;
                        }
                    }
                    Ok(SftpEvent::Question(question)) => {
                        let choice = match (run.abandoned, question.kind()) {
                            (true, _) => TransferChoice::Cancel,
                            (_, TransferQuestionKind::Conflict) => TransferChoice::Overwrite,
                            (
                                _,
                                TransferQuestionKind::Resume | TransferQuestionKind::InvalidResume,
                            ) => TransferChoice::Restart,
                            (_, TransferQuestionKind::Error) => {
                                // 写上是哪一项：Agent 要知道哪个文件没传成。
                                run.failures.push(format!(
                                    "{}: {}",
                                    question.path(),
                                    describe_reason_link(question.message())
                                ));
                                TransferChoice::Skip
                            }
                        };
                        let _ = commands.send_blocking(SftpCommand::Answer {
                            request_id: question.id(),
                            answer: TransferAnswer::new(
                                choice,
                                question.kind() == TransferQuestionKind::Conflict,
                            ),
                        });
                    }
                    Ok(SftpEvent::Prompt(prompt)) => run.refuse(&commands, &prompt),
                    Ok(SftpEvent::Notice(notice)) => run.notice = Some(notice),
                    Ok(SftpEvent::Disconnected(reason)) => run.notice = Some(reason),
                    Ok(SftpEvent::Idle) | Err(_) => break,
                    Ok(_) => {}
                }
            }
            run.summary()
        })();

        let _ = commands.send_blocking(SftpCommand::Shutdown);
        let _ = worker.join();
        result
    }
}

impl CliBackend for SshCliBackend {
    fn exec(
        &self,
        target: &CliTarget,
        command: &str,
        output: &mut dyn FnMut(ExecStream, &[u8]) -> io::Result<()>,
    ) -> Result<i32, CliError> {
        let config = SshConnectionConfig::from(target.login());
        match micaterm_ssh::run_command(&self.connector, &config, command, output) {
            Ok(ExecExit::Code(code)) => Ok(i32::try_from(code).unwrap_or(255)),
            Ok(ExecExit::Signal(signal)) => {
                let _ = output(
                    ExecStream::Stderr,
                    format!("micaterm: {}\n", t!("cli.exec.signal", signal = signal)).as_bytes(),
                );
                Ok(signal_exit_code(&signal))
            }
            // 快照说用不到保险库，读私钥时却撞上锁（快照之后才锁上）：照样说锁着。
            Err(error) if error.error.chain().any(|cause| cause.is::<SecretsLocked>()) => {
                Err(vault_locked())
            }
            Err(error) => Err(CliError::new(
                match error.kind {
                    ExecErrorKind::HostKeyUnknown => ErrorCode::HostKeyUnknown,
                    ExecErrorKind::HostKeyChanged => ErrorCode::HostKeyChanged,
                    ExecErrorKind::MissingCredential => ErrorCode::MissingCredential,
                    ExecErrorKind::Connect | ExecErrorKind::Aborted => ErrorCode::ConnectFailed,
                },
                describe_exec_error(&error),
            )),
        }
    }

    fn upload(
        &self,
        target: &CliTarget,
        source: &Path,
        destination: &str,
        progress: &mut dyn FnMut(TransferCounters) -> io::Result<()>,
    ) -> Result<TransferSummary, CliError> {
        let request = UploadRequest::scp(source.to_path_buf(), remote_path(destination)?);
        self.transfer(target, SftpCommand::Upload(request), progress)
    }

    fn download(
        &self,
        target: &CliTarget,
        source: &str,
        destination: &Path,
        progress: &mut dyn FnMut(TransferCounters) -> io::Result<()>,
    ) -> Result<TransferSummary, CliError> {
        let request = DownloadRequest::scp(remote_path(source)?, destination.to_path_buf())
            .map_err(|error| CliError::new(ErrorCode::BadRequest, describe_sftp_error(&error)))?;
        self.transfer(target, SftpCommand::Download(request), progress)
    }

    fn sync(
        &self,
        target: &CliTarget,
        source: &Path,
        destination: &str,
        delete: bool,
        progress: &mut dyn FnMut(TransferCounters) -> io::Result<()>,
    ) -> Result<TransferSummary, CliError> {
        if !source.is_dir() {
            return Err(CliError::new(
                ErrorCode::BadRequest,
                t!("cli.transfer.no_local_folder", path = source.display()),
            ));
        }
        let request = UploadRequest::sync(source.to_path_buf(), remote_path(destination)?, delete);
        self.transfer(target, SftpCommand::Upload(request), progress)
    }
}

/// 一次传输一路上碰到的事。
#[derive(Default)]
struct Transfer {
    last: Option<TransferProgress>,
    failures: Vec<String>,
    /// 登录走不下去的原因，从它本来要问的那个问题来。
    refused: Option<CliError>,
    notice: Option<String>,
    /// 命令那一端走了，传输正在取消。
    abandoned: bool,
}

impl Transfer {
    /// 只有人才能答的问题一律拒绝，记下为什么。
    fn refuse(&mut self, commands: &async_channel::Sender<SftpCommand>, prompt: &ConnectionPrompt) {
        let refused = match prompt.kind() {
            ConnectionPromptKind::UnknownHost(_) => CliError::new(
                ErrorCode::HostKeyUnknown,
                t!("cli.transfer.host_key_unknown"),
            ),
            ConnectionPromptKind::HostKeyChanged(_) => CliError::new(
                ErrorCode::HostKeyChanged,
                t!("cli.transfer.host_key_changed"),
            ),
            ConnectionPromptKind::Authentication(_) => CliError::new(
                ErrorCode::MissingCredential,
                t!("cli.transfer.missing_credential"),
            ),
        };
        self.refused.get_or_insert(refused);
        let _ = commands.send_blocking(SftpCommand::PromptReply {
            request_id: prompt.request_id(),
            reply: ConnectionPromptReply::Cancel,
        });
    }

    fn failure(&mut self, reason: String) -> CliError {
        self.refused
            .take()
            .unwrap_or_else(|| CliError::new(ErrorCode::ConnectFailed, reason))
    }

    fn summary(&mut self) -> Result<TransferSummary, CliError> {
        match self.last.take() {
            Some(state) if state.phase() == TransferPhase::Completed => Ok(TransferSummary {
                files: state.succeeded() as u64,
                bytes: state.completed_bytes(),
                skipped: state.skipped() as u64,
                failed: state.failed() as u64,
                deleted: state.deleted() as u64,
                failures: std::mem::take(&mut self.failures),
            }),
            _ => Err(self.refused.take().unwrap_or_else(|| {
                CliError::new(
                    ErrorCode::TransferFailed,
                    self.notice
                        .take()
                        .unwrap_or_else(|| t!("cli.transfer.unfinished").to_string()),
                )
            })),
        }
    }
}

fn counters(state: &TransferProgress) -> TransferCounters {
    TransferCounters {
        files: (state.succeeded() + state.skipped() + state.failed()) as u64,
        total_files: state.total() as u64,
        bytes: state.completed_bytes(),
        total_bytes: state.total_bytes(),
    }
}

/// 照人敲的样子读远程路径。`~` 是登录目录（SFTP 的相对路径本来就从那里算）；
/// 光一个名字前面补 `./`，好让它的上级是登录目录而不是根目录。
fn remote_path(path: &str) -> Result<RemotePath, CliError> {
    let path = path.trim();
    let path = match path {
        "" => {
            return Err(CliError::new(
                ErrorCode::BadRequest,
                t!("cli.transfer.empty_remote_path"),
            ));
        }
        "~" => ".".to_string(),
        _ if path.starts_with("~/") => format!(".{}", &path[1..]),
        _ if path.starts_with('/') || path.starts_with("./") || path == "." => path.to_string(),
        _ => format!("./{path}"),
    };
    RemotePath::new(path)
        .map_err(|error| CliError::new(ErrorCode::BadRequest, describe_sftp_error(&error)))
}

/// 被信号杀掉的命令照 shell 的规矩报：128 加信号的编号。
fn signal_exit_code(signal: &str) -> i32 {
    let number = match signal.trim_start_matches("SIG") {
        "HUP" => 1,
        "INT" => 2,
        "QUIT" => 3,
        "ILL" => 4,
        "ABRT" => 6,
        "FPE" => 8,
        "KILL" => 9,
        "USR1" => 10,
        "SEGV" => 11,
        "PIPE" => 13,
        "ALRM" => 14,
        "TERM" => 15,
        _ => return 255,
    };
    128 + number
}

#[cfg(test)]
mod tests {
    use super::{remote_path, signal_exit_code};

    #[test]
    fn remote_paths_read_like_scp() {
        let path = |input: &str| remote_path(input).unwrap().as_str().to_string();
        assert_eq!(path("/var/log"), "/var/log");
        assert_eq!(path("~"), ".");
        assert_eq!(path("~/app.tar.gz"), "./app.tar.gz");
        assert_eq!(path("release"), "./release");
        assert_eq!(path("./release"), "./release");
        assert!(remote_path("").is_err());
    }

    #[test]
    fn a_signal_exits_the_way_a_shell_reports_it() {
        assert_eq!(signal_exit_code("TERM"), 143);
        assert_eq!(signal_exit_code("KILL"), 137);
        assert_eq!(signal_exit_code("Custom(\"XCPU\")"), 255);
    }
}
