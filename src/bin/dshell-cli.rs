use std::{
    ffi::OsString,
    io::{Read as _, Write as _},
    path::PathBuf,
    process::ExitCode,
};

use anyhow::{Context as _, Result, bail};
use clap::{Args, Parser, Subcommand};
use dshell::ssh::{self, Authentication};
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
}

#[derive(Args)]
struct ExecArgs {
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
    #[arg(last = true, required = true)]
    command: OsString,
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
    let Cli {
        command: Command::Exec(args),
    } = Cli::parse();

    let auth_modes = usize::from(args.identity_file.is_some())
        + usize::from(args.agent)
        + usize::from(args.password_stdin);
    if auth_modes != 1 {
        bail!("Choose exactly one of --identity-file, --agent, or --password-stdin.");
    }
    if args.passphrase_stdin && args.identity_file.is_none() {
        bail!("--passphrase-stdin requires --identity-file.");
    }

    let authentication = if let Some(identity_file) = args.identity_file {
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
    let command = args
        .command
        .into_string()
        .map_err(|_| anyhow::anyhow!("the remote command must be valid UTF-8"))?;
    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .context("could not start the SSH runtime")?;
    let output = runtime.block_on(ssh::run_remote_command(
        args.host,
        args.port,
        args.user,
        authentication,
        command,
        args.known_hosts,
    ))?;

    let mut stdout = std::io::stdout().lock();
    stdout.write_all(&output.stdout)?;
    stdout.flush()?;
    let mut stderr = std::io::stderr().lock();
    stderr.write_all(&output.stderr)?;
    stderr.flush()?;
    Ok(output.exit_status.unwrap_or(255).min(255) as u8)
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
        let super::Command::Exec(args) = parsed.command;
        assert_eq!(args.host, "server.example.com");
        assert_eq!(args.user, "operator");
        assert!(args.agent);
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
}
