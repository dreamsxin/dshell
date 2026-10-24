use std::{
    ffi::OsString,
    fs,
    io::Write as _,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result, bail};
use rand::rng;
use russh::keys::{Algorithm, PrivateKey, ssh_key::LineEnding};
use zeroize::Zeroizing;

pub struct GeneratedKey {
    pub private_path: PathBuf,
    pub public_path: PathBuf,
    pub public_key: String,
}

pub fn generate_ed25519(path: &Path, passphrase: &str, comment: &str) -> Result<GeneratedKey> {
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
    let mut key = PrivateKey::random(&mut rng, Algorithm::Ed25519)
        .context("Could not generate the Ed25519 private key")?;
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
    use super::generate_ed25519;
    use russh::keys::PrivateKey;

    #[test]
    fn generates_encrypted_ed25519_key_and_public_file() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("id_ed25519");
        let generated = generate_ed25519(&path, "correct horse", "operator@dshell").unwrap();

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
        assert!(generate_ed25519(&path, "", "").is_err());
    }

    #[test]
    fn generates_unencrypted_key_when_passphrase_is_empty() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested").join("id_ed25519");
        let generated = generate_ed25519(&path, "", "").unwrap();

        let pem = std::fs::read_to_string(&generated.private_path).unwrap();
        let key = PrivateKey::from_openssh(&pem).unwrap();
        assert!(!key.is_encrypted());
        assert!(!generated.public_key.ends_with(' '));
    }
}
