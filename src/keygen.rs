use std::{
    ffi::OsString,
    fs,
    io::Write as _,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context as _, Result, bail};
use rand::rng;
use russh::keys::{
    Algorithm, EcdsaCurve, PrivateKey,
    ssh_key::{LineEnding, private::RsaKeypair},
};
use zeroize::Zeroizing;

pub struct GeneratedKey {
    pub private_path: PathBuf,
    pub public_path: PathBuf,
    pub public_key: String,
}

/// A locally discovered OpenSSH identity file.
///
/// Discovery only inspects directory entries and optional public-key files;
/// it never reads private-key contents.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExistingKey {
    pub private_path: PathBuf,
    pub public_path: Option<PathBuf>,
    pub public_key: Option<String>,
    pub algorithm: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KeyAlgorithm {
    #[default]
    Ed25519,
    Rsa3072,
    Rsa4096,
    EcdsaP256,
    EcdsaP384,
}

impl KeyAlgorithm {
    pub const ALL: [Self; 5] = [
        Self::Ed25519,
        Self::Rsa3072,
        Self::Rsa4096,
        Self::EcdsaP256,
        Self::EcdsaP384,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Ed25519 => "Ed25519",
            Self::Rsa3072 => "RSA 3072",
            Self::Rsa4096 => "RSA 4096",
            Self::EcdsaP256 => "ECDSA P-256",
            Self::EcdsaP384 => "ECDSA P-384",
        }
    }

    pub const fn path_hint(self) -> &'static str {
        match self {
            Self::Ed25519 => "~/.ssh/id_ed25519",
            Self::Rsa3072 | Self::Rsa4096 => "~/.ssh/id_rsa",
            Self::EcdsaP256 | Self::EcdsaP384 => "~/.ssh/id_ecdsa",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Ed25519 => "Modern default, small keys and fast signatures.",
            Self::Rsa3072 => "Compatible RSA key with a 3072-bit modulus.",
            Self::Rsa4096 => "Larger RSA key with a 4096-bit modulus.",
            Self::EcdsaP256 => "NIST P-256 ECDSA key.",
            Self::EcdsaP384 => "NIST P-384 ECDSA key.",
        }
    }

    fn ssh_algorithm(self) -> Algorithm {
        match self {
            Self::Ed25519 => Algorithm::Ed25519,
            Self::Rsa3072 | Self::Rsa4096 => Algorithm::Rsa { hash: None },
            Self::EcdsaP256 => Algorithm::Ecdsa {
                curve: EcdsaCurve::NistP256,
            },
            Self::EcdsaP384 => Algorithm::Ecdsa {
                curve: EcdsaCurve::NistP384,
            },
        }
    }
}

pub fn generate(
    path: &Path,
    passphrase: &str,
    comment: &str,
    algorithm: KeyAlgorithm,
) -> Result<GeneratedKey> {
    if path.as_os_str().is_empty() {
        bail!("A private key file path is required.");
    }
    let public_path = public_key_path(path);
    if path.exists() {
        bail!("{} already exists.", path.display());
    }
    if public_path.exists() {
        bail!("{} already exists.", public_path.display());
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .with_context(|| format!("Could not create the key directory {}", parent.display()))?;
    }

    let mut rng = rng();
    let mut key = match algorithm {
        KeyAlgorithm::Rsa3072 | KeyAlgorithm::Rsa4096 => {
            let bits = if algorithm == KeyAlgorithm::Rsa4096 {
                4096
            } else {
                3072
            };
            PrivateKey::from(
                RsaKeypair::random(&mut rng, bits)
                    .with_context(|| format!("Could not generate the {} key", algorithm.label()))?,
            )
        }
        other => PrivateKey::random(&mut rng, other.ssh_algorithm())
            .with_context(|| format!("Could not generate the {} key", other.label()))?,
    };
    if !comment.trim().is_empty() {
        key.set_comment(comment.trim());
    }
    let public_key = key
        .public_key()
        .to_openssh()
        .context("Could not encode the public key")?;
    let key = if passphrase.is_empty() {
        key
    } else {
        key.encrypt(&mut rng, passphrase)
            .context("Could not encrypt the private key")?
    };
    let pem: Zeroizing<String> = key
        .to_openssh(LineEnding::LF)
        .context("Could not encode the private key")?;

    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("Could not create {}", path.display()))?;
    file.write_all(pem.as_bytes())
        .with_context(|| format!("Could not write {}", path.display()))?;
    drop(file);

    if let Err(error) = fs::write(&public_path, format!("{public_key}\n")) {
        let _ = fs::remove_file(path);
        return Err(error).with_context(|| format!("Could not write {}", public_path.display()));
    }

    Ok(GeneratedKey {
        private_path: path.to_owned(),
        public_path,
        public_key,
    })
}

/// Replace the passphrase on an existing OpenSSH private key.
///
/// The old passphrase is checked before any write. The new passphrase may be
/// empty, which writes the key without encryption. The replacement is staged
/// beside the original and then atomically swapped into place.
pub fn change_passphrase(path: &Path, old_passphrase: &str, new_passphrase: &str) -> Result<()> {
    let pem = Zeroizing::new(
        fs::read_to_string(path)
            .with_context(|| format!("Could not read private key {}", path.display()))?,
    );
    let parsed = PrivateKey::from_openssh(&*pem)
        .with_context(|| format!("Could not parse private key {}", path.display()))?;
    let decrypted = if parsed.is_encrypted() {
        parsed
            .decrypt(old_passphrase)
            .with_context(|| format!("Could not decrypt private key {}", path.display()))?
    } else {
        if !old_passphrase.is_empty() {
            bail!("The private key is not encrypted.");
        }
        parsed
    };
    let comment = decrypted.comment().to_string();

    let mut rng = rng();
    let mut updated = if new_passphrase.is_empty() {
        decrypted
    } else {
        decrypted
            .encrypt(&mut rng, new_passphrase)
            .context("Could not encrypt private key")?
    };
    updated.set_comment(comment);
    let encoded = updated
        .to_openssh(LineEnding::LF)
        .context("Could not encode private key")?;
    let temporary = temporary_key_path(path);
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).with_context(|| {
            format!(
                "Could not create temporary private key {}",
                temporary.display()
            )
        })?;
        file.write_all(encoded.as_bytes()).with_context(|| {
            format!(
                "Could not write temporary private key {}",
                temporary.display()
            )
        })?;
        file.sync_all().with_context(|| {
            format!(
                "Could not flush temporary private key {}",
                temporary.display()
            )
        })?;
        drop(file);
        replace_file(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn temporary_key_path(path: &Path) -> PathBuf {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("identity");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    parent.join(format!(".{name}.dshell-passphrase-{timestamp}.tmp"))
}

fn replace_file(temporary: &Path, destination: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt as _;
        use windows_sys::Win32::Storage::FileSystem::{
            MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
        };

        let temporary: Vec<u16> = temporary
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let destination: Vec<u16> = destination
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let result = unsafe {
            MoveFileExW(
                temporary.as_ptr(),
                destination.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        };
        if result == 0 {
            return Err(std::io::Error::last_os_error()).with_context(|| {
                format!(
                    "Could not replace private key {}",
                    destination_name(destination.as_slice())
                )
            });
        }
        Ok(())
    }

    #[cfg(not(windows))]
    {
        fs::rename(temporary, destination)
            .with_context(|| format!("Could not replace private key {}", destination.display()))?;
        Ok(())
    }
}

#[cfg(windows)]
fn destination_name(path: &[u16]) -> String {
    String::from_utf16_lossy(path)
        .trim_end_matches('\0')
        .to_owned()
}

/// Discover likely OpenSSH private-key files in a user's `.ssh` directory.
///
/// The function deliberately uses conservative filename rules and ignores
/// metadata files. A malformed or unreadable sibling `.pub` file does not
/// prevent the private-key candidate from being listed.
pub fn discover(directory: &Path) -> Result<Vec<ExistingKey>> {
    if !directory.exists() {
        return Ok(Vec::new());
    }
    if !directory.is_dir() {
        bail!("{} is not a directory.", directory.display());
    }

    let mut entries = fs::read_dir(directory)
        .with_context(|| format!("Could not read the key directory {}", directory.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());

    let mut keys = Vec::new();
    for entry in entries {
        let path = entry.path();
        if !entry.file_type()?.is_file() || !is_private_key_candidate(&path) {
            continue;
        }

        let public_path = public_key_path(&path);
        let public_key = fs::read_to_string(&public_path).ok().and_then(|contents| {
            contents
                .lines()
                .map(str::trim)
                .find(|line| !line.is_empty() && !line.starts_with('#'))
                .map(str::to_owned)
        });
        let algorithm = public_key
            .as_deref()
            .and_then(|line| line.split_whitespace().next())
            .unwrap_or("OpenSSH")
            .to_owned();

        keys.push(ExistingKey {
            private_path: path,
            public_path: public_path.exists().then_some(public_path),
            public_key,
            algorithm,
        });
    }
    Ok(keys)
}

fn is_private_key_candidate(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if matches!(
        name,
        "config" | "known_hosts" | "known_hosts.old" | "authorized_keys"
    ) || name.ends_with(".pub")
        || name.ends_with("-cert.pub")
    {
        return false;
    }

    name.starts_with("id_")
        || path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("pem"))
}

fn public_key_path(path: &Path) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(".pub");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{KeyAlgorithm, change_passphrase, discover, generate};
    use russh::keys::PrivateKey;

    #[test]
    fn generates_encrypted_ed25519_key_and_public_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("id_ed25519");
        let generated = generate(
            &path,
            "correct horse",
            "operator@dshell",
            KeyAlgorithm::Ed25519,
        )
        .unwrap();

        assert_eq!(generated.private_path, path);
        assert_eq!(
            generated.public_path,
            directory.path().join("id_ed25519.pub")
        );
        assert!(generated.public_key.starts_with("ssh-ed25519 "));
        assert!(generated.public_key.ends_with(" operator@dshell"));

        let pem = std::fs::read_to_string(&path).unwrap();
        assert!(pem.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"));
        let key = PrivateKey::from_openssh(&pem).unwrap();
        assert!(key.is_encrypted());
        assert!(key.decrypt("correct horse").is_ok());
        assert!(key.decrypt("wrong").is_err());
        assert!(russh::keys::load_secret_key(&path, Some("correct horse")).is_ok());
        assert!(russh::keys::load_secret_key(&path, None).is_err());

        let public = std::fs::read_to_string(&generated.public_path).unwrap();
        assert_eq!(public.trim(), generated.public_key);
        assert!(generate(&path, "", "", KeyAlgorithm::Ed25519).is_err());
    }

    #[test]
    fn generates_unencrypted_key_when_passphrase_is_empty() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested").join("id_ed25519");
        let generated = generate(&path, "", "", KeyAlgorithm::Ed25519).unwrap();

        let pem = std::fs::read_to_string(&generated.private_path).unwrap();
        let key = PrivateKey::from_openssh(&pem).unwrap();
        assert!(!key.is_encrypted());
        assert!(!generated.public_key.ends_with(' '));
    }

    #[test]
    fn changes_passphrase_without_touching_the_file_on_failure() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("id_ed25519");
        let generated = generate(
            &path,
            "old passphrase",
            "operator@dshell",
            KeyAlgorithm::Ed25519,
        )
        .unwrap();
        let original = fs::read(&path).unwrap();

        assert!(change_passphrase(&path, "wrong passphrase", "new passphrase").is_err());
        assert_eq!(fs::read(&path).unwrap(), original);

        change_passphrase(&path, "old passphrase", "new passphrase").unwrap();
        let pem = fs::read_to_string(&path).unwrap();
        let key = PrivateKey::from_openssh(&pem).unwrap();
        assert!(key.is_encrypted());
        assert!(key.decrypt("old passphrase").is_err());
        let decrypted = key.decrypt("new passphrase").unwrap();
        assert_eq!(
            decrypted.public_key().to_openssh().unwrap(),
            generated.public_key
        );

        change_passphrase(&path, "new passphrase", "").unwrap();
        let pem = fs::read_to_string(&path).unwrap();
        let key = PrivateKey::from_openssh(&pem).unwrap();
        assert!(!key.is_encrypted());
    }

    #[test]
    fn adds_passphrase_to_an_unencrypted_key_only_with_an_empty_old_value() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("id_ed25519");
        generate(&path, "", "operator@dshell", KeyAlgorithm::Ed25519).unwrap();
        let original = fs::read(&path).unwrap();

        assert!(change_passphrase(&path, "unexpected", "new passphrase").is_err());
        assert_eq!(fs::read(&path).unwrap(), original);

        change_passphrase(&path, "", "new passphrase").unwrap();
        let pem = fs::read_to_string(&path).unwrap();
        let key = PrivateKey::from_openssh(&pem).unwrap();
        assert!(key.is_encrypted());
        assert!(key.decrypt("new passphrase").is_ok());
    }

    #[test]
    fn generates_rsa_and_ecdsa_keys() {
        let directory = tempfile::tempdir().unwrap();
        let rsa_path = directory.path().join("id_rsa");
        let rsa = generate(
            &rsa_path,
            "rsa passphrase",
            "rsa@dshell",
            KeyAlgorithm::Rsa3072,
        )
        .unwrap();
        assert!(rsa.public_key.starts_with("ssh-rsa "));
        let pem = std::fs::read_to_string(&rsa.private_path).unwrap();
        let key = PrivateKey::from_openssh(&pem).unwrap();
        assert!(key.decrypt("rsa passphrase").is_ok());
        assert!(russh::keys::load_secret_key(&rsa_path, Some("rsa passphrase")).is_ok());

        for (algorithm, prefix, file) in [
            (
                KeyAlgorithm::EcdsaP256,
                "ecdsa-sha2-nistp256 ",
                "id_ecdsa_256",
            ),
            (
                KeyAlgorithm::EcdsaP384,
                "ecdsa-sha2-nistp384 ",
                "id_ecdsa_384",
            ),
        ] {
            let path = directory.path().join(file);
            let generated = generate(&path, "", "ecdsa@dshell", algorithm).unwrap();
            assert!(
                generated.public_key.starts_with(prefix),
                "{}",
                generated.public_key
            );
            assert!(russh::keys::load_secret_key(&path, None).is_ok());
        }
    }

    #[test]
    fn discovers_private_keys_without_reading_metadata_files() {
        let directory = tempfile::tempdir().unwrap();
        let ed25519 = directory.path().join("id_ed25519");
        let generated = generate(&ed25519, "", "operator@dshell", KeyAlgorithm::Ed25519).unwrap();
        fs::write(
            directory.path().join("id_ed25519.pub"),
            format!("# comment\n{}\n", generated.public_key),
        )
        .unwrap();
        fs::write(directory.path().join("config"), "Host *\n").unwrap();
        fs::write(
            directory.path().join("known_hosts"),
            "localhost ssh-ed25519 key\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("authorized_keys"),
            "ssh-ed25519 key\n",
        )
        .unwrap();
        fs::write(directory.path().join("readme.txt"), "not a key\n").unwrap();
        fs::create_dir(directory.path().join("nested")).unwrap();

        let keys = discover(directory.path()).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].private_path, ed25519);
        assert_eq!(
            keys[0].public_path,
            Some(directory.path().join("id_ed25519.pub"))
        );
        assert_eq!(keys[0].algorithm, "ssh-ed25519");
        assert_eq!(
            keys[0].public_key.as_deref(),
            Some(generated.public_key.as_str())
        );
    }

    #[test]
    fn discovers_pem_candidates_and_handles_missing_directory() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("server.pem"), "private key\n").unwrap();
        fs::write(
            directory.path().join("server.pem.pub"),
            "ssh-rsa AAAA comment\n",
        )
        .unwrap();
        fs::write(
            directory.path().join("id_ecdsa-cert.pub"),
            "ssh-rsa AAAA cert\n",
        )
        .unwrap();

        let keys = discover(directory.path()).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].algorithm, "ssh-rsa");
        assert!(
            discover(&directory.path().join("missing"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn rejects_non_directory_discovery_path() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("id_ed25519");
        fs::write(&file, "private key\n").unwrap();
        assert!(discover(&file).is_err());
    }
}
