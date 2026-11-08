use std::{
    ffi::OsString,
    io::{Read as _, Write as _},
    path::PathBuf,
    process::ExitCode,
};

use anyhow::{Context as _, Result, bail};
use clap::{Args, Parser, Subcommand};
use dshell::{
    sftp::SftpCommand,
    ssh::{self, Authentication, RemoteSftpOutput},
};
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
    Ls {
        #[arg(default_value = ".")]
        remote: String,
    },
    Put {
        local: PathBuf,
        remote: String,
        #[arg(long, conflicts_with = "resume")]
        overwrite: bool,
        #[arg(long)]
        resume: bool,
    },
    Get {
        remote: String,
        local: PathBuf,
        #[arg(long, conflicts_with = "resume")]
        overwrite: bool,
        #[arg(long)]
        resume: bool,
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
    let command = match args.action {
        SftpAction::Ls { remote } => SftpCommand::List(remote),
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
    };
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .context("could not start the SSH runtime")?;
    let output = runtime.block_on(ssh::run_sftp_command(
        args.connection.host,
        args.connection.port,
        args.connection.user,
        authentication,
        command,
        args.connection.known_hosts,
    ))?;
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
        RemoteSftpOutput::Completed { transfers, actions } => {
            let mut stderr = std::io::stderr().lock();
            for action in actions {
                writeln!(stderr, "{action}")?;
            }
            for (name, bytes) in transfers {
                writeln!(stderr, "{name}: {bytes} bytes transferred")?;
            }
            stderr.flush()?;
        }
    }
    Ok(0)
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
    fn remote_file_names_cannot_emit_terminal_control_sequences() {
        assert_eq!(
            super::escape_terminal_controls("normal\u{1b}[31mname\n"),
            "normal\\u{1b}[31mname\\n"
        );
    }
}
