use std::{
    ffi::OsString,
    fs,
    io::Write as _,
    path::{Path, PathBuf},
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

fn public_key_path(path: &Path) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(".pub");
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::{KeyAlgorithm, generate};
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
}
