use std::{
    ffi::OsString,
    io::{Read as _, Write as _},
    path::PathBuf,
    process::ExitCode,
};

use anyhow::{Context as _, Result, bail};
use clap::{Args, Parser, Subcommand};
use dshell::{
    sftp::{self, SftpCommand},
    ssh::{self, Authentication, RemoteSftpOutput},
};
use serde::Deserialize;
use tokio::runtime::Builder;
use zeroize::{Zeroize as _, Zeroizing};

#[derive(Parser)]
#[command(
    name = "dshell-cli",
    version,
    about = "Run SSH commands without opening the desktop UI"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Exec(ExecArgs),
    Sftp(SftpArgs),
}

#[derive(Args)]
struct ExecArgs {
    #[command(flatten)]
    connection: ConnectionArgs,
    #[arg(
        long,
        help = "Temporarily expose the local SSH agent to the remote command"
    )]
    forward_agent: bool,
    #[arg(last = true, required = true)]
    command: OsString,
}

#[derive(Args)]
struct SftpArgs {
    #[command(flatten)]
    connection: ConnectionArgs,
    #[command(subcommand)]
    action: SftpAction,
}

#[derive(Args)]
struct ConnectionArgs {
    #[arg(long)]
    host: String,
    #[arg(long)]
    user: String,
    #[arg(long, default_value_t = 22)]
    port: u16,
    #[arg(long, value_name = "PATH")]
    identity_file: Option<PathBuf>,
    #[arg(long)]
    agent: bool,
    #[arg(long)]
    password_stdin: bool,
    #[arg(long, requires = "identity_file")]
    passphrase_stdin: bool,
    #[arg(long, value_name = "PATH")]
    known_hosts: Option<PathBuf>,
}

#[derive(Subcommand)]
enum SftpAction {
    Batch {
        #[arg(long, value_name = "JSON")]
        file: PathBuf,
    },
    PutGlob {
        local_pattern: String,
        remote_directory: String,
        #[arg(long, conflicts_with = "resume")]
        overwrite: bool,
        #[arg(long)]
        resume: bool,
    },
    GetGlob {
        remote_pattern: String,
        local_directory: PathBuf,
        #[arg(long, conflicts_with = "resume")]
        overwrite: bool,
        #[arg(long)]
        resume: bool,
    },
    Ls {
        #[arg(default_value = ".")]
        remote: String,
    },
    Stat {
        remote: String,
    },
    #[command(name = "realpath")]
    RealPath {
        remote: String,
    },
    Rename {
        from: String,
        to: String,
    },
    Chmod {
        mode: String,
        remote: String,
    },
    Readlink {
        remote: String,
    },
    Symlink {
        target: String,
        link: String,
    },
    Put {
        local: PathBuf,
        remote: String,
        #[arg(long, conflicts_with = "resume")]
        overwrite: bool,
        #[arg(long)]
        resume: bool,
    },
    PutText {
        local: PathBuf,
        remote: String,
        #[arg(long)]
        overwrite: bool,
    },
    Get {
        remote: String,
        local: PathBuf,
        #[arg(long, conflicts_with = "resume")]
        overwrite: bool,
        #[arg(long)]
        resume: bool,
    },
    GetText {
        remote: String,
        local: PathBuf,
        #[arg(long)]
        overwrite: bool,
    },
    PutDir {
        local: PathBuf,
        remote: String,
        #[arg(long, conflicts_with = "resume")]
        overwrite: bool,
        #[arg(long)]
        resume: bool,
    },
    GetDir {
        remote: String,
        local: PathBuf,
        #[arg(long, conflicts_with = "resume")]
        overwrite: bool,
        #[arg(long)]
        resume: bool,
    },
    Mkdir {
        remote: String,
    },
    Rm {
        remote: String,
    },
    Rmdir {
        remote: String,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SftpBatchFile {
    version: u8,
    operations: Vec<SftpBatchOperation>,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum SftpBatchOperation {
    List {
        #[serde(default = "default_remote_path")]
        remote: String,
    },
    Stat {
        remote: String,
    },
    RealPath {
        remote: String,
    },
    Rename {
        from: String,
        to: String,
    },
    Chmod {
        remote: String,
        mode: String,
    },
    Readlink {
        remote: String,
    },
    Symlink {
        target: String,
        link: String,
    },
    Put {
        local: PathBuf,
        remote: String,
        #[serde(default)]
        overwrite: bool,
        #[serde(default)]
        resume: bool,
    },
    PutText {
        local: PathBuf,
        remote: String,
        #[serde(default)]
        overwrite: bool,
    },
    Get {
        remote: String,
        local: PathBuf,
        #[serde(default)]
        overwrite: bool,
        #[serde(default)]
        resume: bool,
    },
    GetText {
        remote: String,
        local: PathBuf,
        #[serde(default)]
        overwrite: bool,
    },
    PutDir {
        local: PathBuf,
        remote: String,
        #[serde(default)]
        overwrite: bool,
        #[serde(default)]
        resume: bool,
    },
    PutGlob {
        local_pattern: String,
        remote_directory: String,
        #[serde(default)]
        overwrite: bool,
        #[serde(default)]
        resume: bool,
    },
    GetDir {
        remote: String,
        local: PathBuf,
        #[serde(default)]
        overwrite: bool,
        #[serde(default)]
        resume: bool,
    },
    GetGlob {
        remote_pattern: String,
        local_directory: PathBuf,
        #[serde(default)]
        overwrite: bool,
        #[serde(default)]
        resume: bool,
    },
    Mkdir {
        remote: String,
    },
    Rm {
        remote: String,
    },
    Rmdir {
        remote: String,
    },
}

const MAX_BATCH_FILE_BYTES: u64 = 1024 * 1024;
const MAX_BATCH_OPERATIONS: usize = 256;

fn default_remote_path() -> String {
    ".".to_owned()
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("dshell-cli: {error:#}");
            ExitCode::from(255)
        }
    }
}

fn run() -> Result<u8> {
    let cli = Cli::parse();
    match cli.command {
        Command::Exec(args) => run_exec(args),
        Command::Sftp(args) => run_sftp(args),
    }
}

fn run_exec(args: ExecArgs) -> Result<u8> {
    let authentication = build_authentication(&args.connection)?;
    let command = args
        .command
        .into_string()
        .map_err(|_| anyhow::anyhow!("the remote command must be valid UTF-8"))?;
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .context("could not start the SSH runtime")?;
    let output = runtime.block_on(ssh::run_remote_command(
        args.connection.host,
        args.connection.port,
        args.connection.user,
        authentication,
        command,
        args.connection.known_hosts,
        args.forward_agent,
    ))?;

    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&output.stdout)?;
    stdout.flush()?;
    let mut stderr = std::io::stderr().lock();
    stderr.write_all(&output.stderr)?;
    stderr.flush()?;
    Ok(output.exit_status.unwrap_or(255).min(255) as u8)
}

fn build_authentication(args: &ConnectionArgs) -> Result<Authentication> {
    let auth_modes = usize::from(args.identity_file.is_some())
        + usize::from(args.agent)
        + usize::from(args.password_stdin);
    if auth_modes != 1 {
        bail!("Choose exactly one of --identity-file, --agent, or --password-stdin.");
    }
    if args.passphrase_stdin && args.identity_file.is_none() {
        bail!("--passphrase-stdin requires --identity-file.");
    }

    let authentication = if let Some(identity_file) = args.identity_file.clone() {
        let passphrase = if args.passphrase_stdin {
            read_secret_stdin("identity passphrase")?
        } else {
            Zeroizing::new(String::new())
        };
        Authentication::PublicKey {
            identity_file,
            passphrase,
        }
    } else if args.password_stdin {
        Authentication::Password(read_secret_stdin("password")?)
    } else {
        Authentication::Agent
    };
    Ok(authentication)
}

fn run_sftp(args: SftpArgs) -> Result<u8> {
    let authentication = build_authentication(&args.connection)?;
    let commands = match args.action {
        SftpAction::Batch { file } => load_sftp_batch(&file)?,
        SftpAction::PutGlob {
            local_pattern,
            remote_directory,
            overwrite,
            resume,
        } => vec![SftpCommand::UploadGlob {
            local_pattern,
            remote_directory,
            overwrite,
            resume,
        }],
        SftpAction::GetGlob {
            remote_pattern,
            local_directory,
            overwrite,
            resume,
        } => vec![SftpCommand::DownloadGlob {
            remote_pattern,
            local_directory,
            overwrite,
            resume,
        }],
        action => vec![match action {
            SftpAction::Ls { remote } => SftpCommand::List(remote),
            SftpAction::Stat { remote } => SftpCommand::Stat(remote),
            SftpAction::RealPath { remote } => SftpCommand::RealPath(remote),
            SftpAction::Rename { from, to } => SftpCommand::Rename { from, to },
            SftpAction::Chmod { mode, remote } => SftpCommand::SetMetadata {
                path: remote,
                permissions: Some(sftp::parse_permission_mode(&mode).map_err(anyhow::Error::msg)?),
                atime: None,
                mtime: None,
            },
            SftpAction::Readlink { remote } => SftpCommand::ReadLink(remote),
            SftpAction::Symlink { target, link } => SftpCommand::CreateSymlink { target, link },
            SftpAction::Put {
                local,
                remote,
                overwrite,
                resume,
            } => SftpCommand::Upload {
                local,
                remote,
                overwrite,
                resume,
            },
            SftpAction::PutText {
                local,
                remote,
                overwrite,
            } => SftpCommand::UploadText {
                local,
                remote,
                overwrite,
            },
            SftpAction::Get {
                remote,
                local,
                overwrite,
                resume,
            } => SftpCommand::Download {
                remote,
                local,
                overwrite,
                resume,
            },
            SftpAction::GetText {
                remote,
                local,
                overwrite,
            } => SftpCommand::DownloadText {
                remote,
                local,
                overwrite,
            },
            SftpAction::PutDir {
                local,
                remote,
                overwrite,
                resume,
            } => SftpCommand::UploadDirectory {
                local,
                remote,
                overwrite,
                resume,
            },
            SftpAction::GetDir {
                remote,
                local,
                overwrite,
                resume,
            } => SftpCommand::DownloadDirectory {
                remote,
                local,
                overwrite,
                resume,
            },
            SftpAction::Mkdir { remote } => SftpCommand::CreateDirectory(remote),
            SftpAction::Rm { remote } => SftpCommand::RemoveFile(remote),
            SftpAction::Rmdir { remote } => SftpCommand::RemoveDirectory(remote),
            SftpAction::PutGlob { .. } | SftpAction::GetGlob { .. } => {
                unreachable!("glob actions are expanded before this match")
            }
            SftpAction::Batch { .. } => unreachable!("batch file was handled above"),
        }],
    };
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .context("could not start the SSH runtime")?;
    let outputs = runtime.block_on(ssh::run_sftp_commands(
        args.connection.host,
        args.connection.port,
        args.connection.user,
        authentication,
        commands,
        args.connection.known_hosts,
    ))?;
    for output in outputs {
        print_sftp_output(output)?;
    }
    Ok(0)
}

fn load_sftp_batch(path: &std::path::Path) -> Result<Vec<SftpCommand>> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("could not inspect SFTP batch file {}", path.display()))?;
    if metadata.len() > MAX_BATCH_FILE_BYTES {
        bail!("SFTP batch file exceeds 1 MiB.");
    }
    let contents = std::fs::read(path)
        .with_context(|| format!("could not read SFTP batch file {}", path.display()))?;
    let document: SftpBatchFile = serde_json::from_slice(&contents)
        .with_context(|| format!("could not parse SFTP batch file {}", path.display()))?;
    if document.version != 1 {
        bail!("SFTP batch version {} is not supported.", document.version);
    }
    if document.operations.is_empty() || document.operations.len() > MAX_BATCH_OPERATIONS {
        bail!("SFTP batch must contain between 1 and 256 operations.");
    }
    document
        .operations
        .into_iter()
        .enumerate()
        .map(|(index, operation)| {
            let command = match operation {
                SftpBatchOperation::List { remote } => SftpCommand::List(remote),
                SftpBatchOperation::Stat { remote } => SftpCommand::Stat(remote),
                SftpBatchOperation::RealPath { remote } => SftpCommand::RealPath(remote),
                SftpBatchOperation::Rename { from, to } => SftpCommand::Rename { from, to },
                SftpBatchOperation::Chmod { remote, mode } => SftpCommand::SetMetadata {
                    path: remote,
                    permissions: Some(
                        sftp::parse_permission_mode(&mode).map_err(anyhow::Error::msg)?,
                    ),
                    atime: None,
                    mtime: None,
                },
                SftpBatchOperation::Readlink { remote } => SftpCommand::ReadLink(remote),
                SftpBatchOperation::Symlink { target, link } => {
                    SftpCommand::CreateSymlink { target, link }
                }
                SftpBatchOperation::Put {
                    local,
                    remote,
                    overwrite,
                    resume,
                } => {
                    validate_batch_transfer_options(overwrite, resume, index)?;
                    SftpCommand::Upload {
                        local,
                        remote,
                        overwrite,
                        resume,
                    }
                }
                SftpBatchOperation::PutText {
                    local,
                    remote,
                    overwrite,
                } => SftpCommand::UploadText {
                    local,
                    remote,
                    overwrite,
                },
                SftpBatchOperation::Get {
                    remote,
                    local,
                    overwrite,
                    resume,
                } => {
                    validate_batch_transfer_options(overwrite, resume, index)?;
                    SftpCommand::Download {
                        remote,
                        local,
                        overwrite,
                        resume,
                    }
                }
                SftpBatchOperation::GetText {
                    remote,
                    local,
                    overwrite,
                } => SftpCommand::DownloadText {
                    remote,
                    local,
                    overwrite,
                },
                SftpBatchOperation::PutDir {
                    local,
                    remote,
                    overwrite,
                    resume,
                } => {
                    validate_batch_transfer_options(overwrite, resume, index)?;
                    SftpCommand::UploadDirectory {
                        local,
                        remote,
                        overwrite,
                        resume,
                    }
                }
                SftpBatchOperation::PutGlob {
                    local_pattern,
                    remote_directory,
                    overwrite,
                    resume,
                } => {
                    validate_batch_transfer_options(overwrite, resume, index)?;
                    SftpCommand::UploadGlob {
                        local_pattern,
                        remote_directory,
                        overwrite,
                        resume,
                    }
                }
                SftpBatchOperation::GetDir {
                    remote,
                    local,
                    overwrite,
                    resume,
                } => {
                    validate_batch_transfer_options(overwrite, resume, index)?;
                    SftpCommand::DownloadDirectory {
                        remote,
                        local,
                        overwrite,
                        resume,
                    }
                }
                SftpBatchOperation::GetGlob {
                    remote_pattern,
                    local_directory,
                    overwrite,
                    resume,
                } => {
                    validate_batch_transfer_options(overwrite, resume, index)?;
                    SftpCommand::DownloadGlob {
                        remote_pattern,
                        local_directory,
                        overwrite,
                        resume,
                    }
                }
                SftpBatchOperation::Mkdir { remote } => SftpCommand::CreateDirectory(remote),
                SftpBatchOperation::Rm { remote } => SftpCommand::RemoveFile(remote),
                SftpBatchOperation::Rmdir { remote } => SftpCommand::RemoveDirectory(remote),
            };
            Ok(command)
        })
        .collect()
}

fn validate_batch_transfer_options(overwrite: bool, resume: bool, index: usize) -> Result<()> {
    if overwrite && resume {
        bail!(
            "SFTP batch operation {} cannot set both overwrite and resume.",
            index + 1
        );
    }
    Ok(())
}

fn print_sftp_output(output: RemoteSftpOutput) -> Result<()> {
    match output {
        RemoteSftpOutput::Listed { path, entries } => {
            let mut stdout = std::io::stdout().lock();
            writeln!(stdout, "{}", path)?;
            for entry in entries {
                let kind = if entry.is_dir { "d" } else { "-" };
                writeln!(
                    stdout,
                    "{kind}\t{}\t{}",
                    entry.size,
                    escape_terminal_controls(&entry.name)
                )?;
            }
            stdout.flush()?;
        }
        RemoteSftpOutput::Stat(entry) => {
            let kind = if entry.is_dir {
                "directory"
            } else if entry.is_symlink {
                "symlink"
            } else if entry.is_file {
                "file"
            } else {
                "other"
            };
            let permissions = entry
                .permissions
                .map_or_else(|| "unknown".to_owned(), |mode| format!("{mode:o}"));
            let mtime = entry
                .mtime
                .map_or_else(|| "unknown".to_owned(), |value| value.to_string());
            let mut stdout = std::io::stdout().lock();
            writeln!(
                stdout,
                "path={}\ttype={kind}\tsize={}\tmode={permissions}\tmtime={mtime}",
                escape_terminal_controls(&entry.path),
                entry.size
            )?;
            stdout.flush()?;
        }
        RemoteSftpOutput::RealPath { resolved_path, .. } => {
            let mut stdout = std::io::stdout().lock();
            writeln!(stdout, "{}", escape_terminal_controls(&resolved_path))?;
            stdout.flush()?;
        }
        RemoteSftpOutput::LinkTarget { target, .. } => {
            let mut stdout = std::io::stdout().lock();
            writeln!(stdout, "{}", escape_terminal_controls(&target))?;
            stdout.flush()?;
        }
        RemoteSftpOutput::Completed { transfers, actions } => {
            let mut stderr = std::io::stderr().lock();
            for action in actions {
                writeln!(stderr, "{}", escape_terminal_controls(&action))?;
            }
            for (name, bytes) in transfers {
                writeln!(
                    stderr,
                    "{}: {bytes} bytes transferred",
                    escape_terminal_controls(&name)
                )?;
            }
            stderr.flush()?;
        }
    }
    Ok(())
}

fn escape_terminal_controls(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                character.escape_default().collect::<String>()
            } else {
                character.to_string()
            }
        })
        .collect()
}

fn read_secret_stdin(label: &str) -> Result<Zeroizing<String>> {
    let mut value = Zeroizing::new(String::new());
    std::io::stdin()
        .take(4098)
        .read_to_string(&mut value)
        .with_context(|| format!("could not read {label} from standard input"))?;
    if value.len() > 4096 {
        value.zeroize();
        bail!("{label} input exceeded 4096 bytes.");
    }
    if value.ends_with('\n') {
        value.pop();
    }
    if value.ends_with('\r') {
        value.pop();
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::Cli;
    use clap::Parser;
    use dshell::sftp::SftpCommand;

    #[test]
    fn exec_parses_authentication_and_command_after_separator() {
        let parsed = Cli::try_parse_from([
            "dshell-cli",
            "exec",
            "--host",
            "server.example.com",
            "--user",
            "operator",
            "--agent",
            "--forward-agent",
            "--",
            "uname -a",
        ])
        .unwrap();
        let super::Command::Exec(args) = parsed.command else {
            panic!("expected exec command");
        };
        assert_eq!(args.connection.host, "server.example.com");
        assert_eq!(args.connection.user, "operator");
        assert!(args.connection.agent);
        assert!(args.forward_agent);
        assert_eq!(args.command, "uname -a");
    }

    #[test]
    fn exec_requires_an_explicit_command_and_authentication_mode() {
        assert!(
            Cli::try_parse_from([
                "dshell-cli",
                "exec",
                "--host",
                "server.example.com",
                "--user",
                "operator",
                "--agent",
            ])
            .is_err()
        );
    }

    #[test]
    fn sftp_parses_recursive_resume_operations() {
        let parsed = Cli::try_parse_from([
            "dshell-cli",
            "sftp",
            "--host",
            "server.example.com",
            "--user",
            "operator",
            "--agent",
            "put-dir",
            "local-assets",
            "/srv/assets",
            "--resume",
        ])
        .unwrap();
        let super::Command::Sftp(args) = parsed.command else {
            panic!("expected SFTP command");
        };
        assert!(matches!(
            args.action,
            super::SftpAction::PutDir { resume: true, .. }
        ));
    }

    #[test]
    fn sftp_parses_read_only_path_queries() {
        for (action, expected) in [("stat", "Stat"), ("realpath", "RealPath")] {
            let parsed = Cli::try_parse_from([
                "dshell-cli",
                "sftp",
                "--host",
                "server.example.com",
                "--user",
                "operator",
                "--agent",
                action,
                "/srv/app/file.txt",
            ])
            .unwrap();
            let super::Command::Sftp(args) = parsed.command else {
                panic!("expected SFTP command");
            };
            match (expected, args.action) {
                ("Stat", super::SftpAction::Stat { remote })
                | ("RealPath", super::SftpAction::RealPath { remote }) => {
                    assert_eq!(remote, "/srv/app/file.txt");
                }
                _ => panic!("unexpected SFTP action"),
            }
        }
    }

    #[test]
    fn sftp_parses_remote_file_management_actions() {
        let actions = [
            ("rename", vec!["/srv/old", "/srv/new"]),
            ("chmod", vec!["0640", "/srv/file"]),
            ("readlink", vec!["/srv/link"]),
            ("symlink", vec!["target", "/srv/link"]),
        ];
        for (action, arguments) in actions {
            let mut command = vec![
                "dshell-cli",
                "sftp",
                "--host",
                "server.example.com",
                "--user",
                "operator",
                "--agent",
                action,
            ];
            command.extend(arguments);
            assert!(
                Cli::try_parse_from(command).is_ok(),
                "{action} should parse"
            );
        }
    }

    #[test]
    fn sftp_parses_text_transfers_with_explicit_overwrite() {
        let parsed = Cli::try_parse_from([
            "dshell-cli",
            "sftp",
            "--host",
            "server.example.com",
            "--user",
            "operator",
            "--agent",
            "put-text",
            "local.txt",
            "/srv/file.txt",
            "--overwrite",
        ])
        .unwrap();
        let super::Command::Sftp(args) = parsed.command else {
            panic!("expected SFTP command");
        };
        assert!(matches!(
            args.action,
            super::SftpAction::PutText {
                overwrite: true,
                ..
            }
        ));
    }

    #[test]
    fn sftp_overwrite_and_resume_are_mutually_exclusive() {
        assert!(
            Cli::try_parse_from([
                "dshell-cli",
                "sftp",
                "--host",
                "server.example.com",
                "--user",
                "operator",
                "--agent",
                "put",
                "local.file",
                "/tmp/remote.file",
                "--overwrite",
                "--resume",
            ])
            .is_err()
        );
    }

    #[test]
    fn sftp_batch_file_preserves_order_and_transfer_modes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("batch.json");
        std::fs::write(
            &path,
            r#"{"version":1,"operations":[{"op":"list"},{"op":"stat","remote":"/tmp/file.bin"},{"op":"real_path","remote":"/tmp"},{"op":"rename","from":"/tmp/old","to":"/tmp/new"},{"op":"chmod","remote":"/tmp/new","mode":"0640"},{"op":"readlink","remote":"/tmp/link"},{"op":"symlink","target":"new","link":"/tmp/link"},{"op":"put_text","local":"file.txt","remote":"/tmp/file.txt"},{"op":"get_text","remote":"/tmp/file.txt","local":"copy.txt","overwrite":true},{"op":"put","local":"file.bin","remote":"/tmp/file.bin","resume":true},{"op":"mkdir","remote":"/tmp/new-dir"}]}"#,
        )
        .unwrap();

        let commands = super::load_sftp_batch(&path).unwrap();
        assert_eq!(commands.len(), 11);
        assert!(matches!(&commands[0], SftpCommand::List(path) if path == "."));
        assert!(matches!(
            &commands[1],
            SftpCommand::Stat(path) if path == "/tmp/file.bin"
        ));
        assert!(matches!(
            &commands[2],
            SftpCommand::RealPath(path) if path == "/tmp"
        ));
        assert!(
            matches!(&commands[3], SftpCommand::Rename { from, to } if from == "/tmp/old" && to == "/tmp/new")
        );
        assert!(matches!(
            &commands[4],
            SftpCommand::SetMetadata { path, permissions: Some(0o640), .. } if path == "/tmp/new"
        ));
        assert!(matches!(&commands[5], SftpCommand::ReadLink(path) if path == "/tmp/link"));
        assert!(
            matches!(&commands[6], SftpCommand::CreateSymlink { target, link } if target == "new" && link == "/tmp/link")
        );
        assert!(matches!(
            &commands[7],
            SftpCommand::UploadText { local, remote, overwrite: false }
                if local == std::path::Path::new("file.txt") && remote == "/tmp/file.txt"
        ));
        assert!(matches!(
            &commands[8],
            SftpCommand::DownloadText { remote, local, overwrite: true }
                if remote == "/tmp/file.txt" && local == std::path::Path::new("copy.txt")
        ));
        assert!(matches!(
            &commands[9],
            SftpCommand::Upload { remote, resume: true, overwrite: false, .. }
                if remote == "/tmp/file.bin"
        ));
        assert!(matches!(
            &commands[10],
            SftpCommand::CreateDirectory(path) if path == "/tmp/new-dir"
        ));
    }

    #[test]
    fn sftp_batch_file_parses_final_segment_glob_transfers() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("glob-batch.json");
        std::fs::write(
            &path,
            r#"{"version":1,"operations":[{"op":"put_glob","local_pattern":"dist/*.zip","remote_directory":"/srv/releases"},{"op":"get_glob","remote_pattern":"/var/log/*.log","local_directory":"logs","resume":true}]}"#,
        )
        .unwrap();
        let commands = super::load_sftp_batch(&path).unwrap();
        assert!(matches!(
            &commands[0],
            SftpCommand::UploadGlob { local_pattern, remote_directory, .. }
                if local_pattern == "dist/*.zip" && remote_directory == "/srv/releases"
        ));
        assert!(matches!(
            &commands[1],
            SftpCommand::DownloadGlob { remote_pattern, resume: true, .. }
                if remote_pattern == "/var/log/*.log"
        ));
    }

    #[test]
    fn sftp_batch_file_rejects_secrets_unknown_fields_and_oversized_batches() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("batch.json");
        std::fs::write(
            &path,
            r#"{"version":1,"operations":[{"op":"list","remote":"/tmp","password":"secret"}]}"#,
        )
        .unwrap();
        assert!(super::load_sftp_batch(&path).is_err());

        std::fs::write(
            &path,
            r#"{"version":1,"operations":[{"op":"chmod","remote":"/tmp/file","mode":"0888"}]}"#,
        )
        .unwrap();
        assert!(super::load_sftp_batch(&path).is_err());

        let operations = std::iter::repeat_n(r#"{"op":"list"}"#, 257)
            .collect::<Vec<_>>()
            .join(",");
        std::fs::write(
            &path,
            format!(r#"{{"version":1,"operations":[{operations}]}}"#),
        )
        .unwrap();
        assert!(super::load_sftp_batch(&path).is_err());
    }

    #[test]
    fn remote_file_names_cannot_emit_terminal_control_sequences() {
        assert_eq!(
            super::escape_terminal_controls("normal\u{1b}[31mname\n"),
            "normal\\u{1b}[31mname\\n"
        );
    }
}
