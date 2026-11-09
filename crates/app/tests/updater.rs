//! Update path from an older build to this one, against a local `latest.json`
//! signed with a throwaway minisign key.
//!
//! On Linux the update is applied to a stand-in AppImage. On Windows
//! `install` starts the NSIS installer and exits, so these tests stop after the
//! signature check; `.github/scripts/test-windows-installers.ps1` runs that
//! installer the way the updater does.
#![cfg(any(target_os = "linux", target_os = "windows"))]

use std::io::{BufRead, BufReader, Cursor, Write};
use std::net::TcpListener;
use std::sync::Arc;

use ado_planner::updater::Updater;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use minisign::KeyPair;

const OLD_VERSION: &str = "0.2.1";
const NEW_VERSION: &str = env!("CARGO_PKG_VERSION");
const NOTES: &str = "### Features\n\n- something new";

struct Key(KeyPair);

impl Key {
    fn generate() -> Self {
        Self(KeyPair::generate_unencrypted_keypair().expect("keypair"))
    }

    /// The `.pub` file `cargo packager signer generate` writes.
    fn public(&self) -> String {
        STANDARD.encode(self.0.pk.to_box().expect("pk box").to_string())
    }

    /// The `.sig` file cargo-packager writes beside each installer.
    fn sign(&self, bytes: &[u8]) -> String {
        let signature =
            minisign::sign(Some(&self.0.pk), &self.0.sk, Cursor::new(bytes), None, None)
                .expect("sign");
        STANDARD.encode(signature.to_string())
    }
}

/// Serves `/latest.json` and `/artifact` until the test process exits.
fn serve(feed: impl Fn(&str) -> String + Send + Sync + 'static, artifact: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let base = format!("http://{}", listener.local_addr().expect("addr"));
    let feed_base = base.clone();
    let feed = Arc::new(feed);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut request_line = String::new();
            if reader.read_line(&mut request_line).is_err() {
                continue;
            }
            let mut line = String::new();
            while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                line.clear();
            }
            let path = request_line.split_whitespace().nth(1).unwrap_or("/");
            let (status, kind, body) = match path {
                "/latest.json" => ("200 OK", "application/json", feed(&feed_base).into_bytes()),
                "/artifact" => ("200 OK", "application/octet-stream", artifact.clone()),
                _ => ("404 Not Found", "text/plain", b"not found".to_vec()),
            };
            let head = format!(
                "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    format!("{base}/latest.json")
}

/// The shape `.github/scripts/write-rust-latest-json.py` publishes.
fn latest_json(base: &str, signature: &str) -> String {
    let url = format!("{base}/artifact");
    let arch = std::env::consts::ARCH;
    serde_json::json!({
        "version": NEW_VERSION,
        "notes": NOTES,
        "pub_date": "2026-11-09T12:00:00Z",
        "platforms": {
            format!("linux-{arch}"): { "url": url, "signature": signature, "format": "appimage" },
            format!("windows-{arch}"): { "url": url, "signature": signature, "format": "nsis" },
        },
    })
    .to_string()
}

fn release(key: &Key, artifact: &[u8], served: Vec<u8>) -> String {
    let signature = key.sign(artifact);
    serve(move |base| latest_json(base, &signature), served)
}

fn download_error(updater: &Updater) -> String {
    match updater.download_blocking(OLD_VERSION) {
        Ok(Some(_)) => panic!("download verified"),
        Ok(None) => panic!("no update offered"),
        Err(err) => err,
    }
}

fn new_build() -> Vec<u8> {
    format!("ado-planner {NEW_VERSION} installer bytes").into_bytes()
}

#[test]
fn older_build_is_offered_this_version_and_verifies_it() {
    let key = Key::generate();
    let artifact = new_build();
    let endpoint = release(&key, &artifact, artifact.clone());
    let mut updater = Updater::new(endpoint, key.public());

    let prompt = updater
        .check_blocking(OLD_VERSION)
        .expect("check")
        .expect("update offered");
    assert_eq!(prompt.version, NEW_VERSION);
    assert_eq!(prompt.release_notes.as_deref(), Some(NOTES));
    assert_eq!(updater.prompt.as_ref(), Some(&prompt));

    let Ok(Some(update)) = updater.download_blocking(OLD_VERSION) else {
        panic!("signed update did not download and verify");
    };
    assert_eq!(update.version(), NEW_VERSION);
    assert_eq!(update.bytes(), artifact.as_slice());
}

#[test]
fn this_version_is_not_offered_itself() {
    let key = Key::generate();
    let artifact = new_build();
    let endpoint = release(&key, &artifact, artifact.clone());
    let mut updater = Updater::new(endpoint, key.public());

    assert!(
        updater
            .check_blocking(NEW_VERSION)
            .expect("check")
            .is_none()
    );
    assert!(updater.check_blocking("999.0.0").expect("check").is_none());
    assert!(updater.prompt.is_none());
}

#[test]
fn tampered_download_is_rejected() {
    let key = Key::generate();
    let artifact = new_build();
    let mut tampered = artifact.clone();
    tampered.extend_from_slice(b" plus a payload");
    let endpoint = release(&key, &artifact, tampered);
    let mut updater = Updater::new(endpoint, key.public());

    assert!(
        updater
            .check_blocking(OLD_VERSION)
            .expect("check")
            .is_some()
    );
    let err = download_error(&updater);
    assert!(err.contains("signature verification failed"), "{err}");
}

#[test]
fn signature_from_another_key_is_rejected() {
    let trusted = Key::generate();
    let other = Key::generate();
    let artifact = new_build();
    let endpoint = release(&other, &artifact, artifact.clone());
    let updater = Updater::new(endpoint, trusted.public());

    let err = download_error(&updater);
    assert!(err.contains("different key"), "{err}");
}

#[cfg(target_os = "linux")]
mod appimage {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn old_appimage(dir: &tempfile::TempDir) -> std::path::PathBuf {
        let path = dir.path().join("ADO Planner.AppImage");
        std::fs::write(&path, format!("ado-planner {OLD_VERSION}")).expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    #[test]
    fn yes_replaces_the_older_appimage_with_the_signed_build() {
        let key = Key::generate();
        let artifact = new_build();
        let endpoint = release(&key, &artifact, artifact.clone());
        let dir = tempfile::tempdir().expect("tempdir");
        let appimage = old_appimage(&dir);
        let mut updater = Updater::new(endpoint, key.public()).with_executable_path(&appimage);

        updater.check_blocking(OLD_VERSION).expect("check");
        updater.apply_from(OLD_VERSION).expect("apply");

        assert_eq!(std::fs::read(&appimage).expect("read"), artifact);
        let mode = std::fs::metadata(&appimage)
            .expect("stat")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
    }

    #[test]
    fn a_rejected_download_leaves_the_older_appimage_alone() {
        let key = Key::generate();
        let artifact = new_build();
        let endpoint = release(&key, &artifact, b"not what was signed".to_vec());
        let dir = tempfile::tempdir().expect("tempdir");
        let appimage = old_appimage(&dir);
        let mut updater = Updater::new(endpoint, key.public()).with_executable_path(&appimage);

        updater.check_blocking(OLD_VERSION).expect("check");
        let err = updater.apply_from(OLD_VERSION).expect_err("apply");
        assert!(err.contains("signature verification failed"), "{err}");

        let expected = format!("ado-planner {OLD_VERSION}");
        assert_eq!(std::fs::read_to_string(&appimage).expect("read"), expected);
    }

    #[test]
    fn later_skips_the_install() {
        let key = Key::generate();
        let artifact = new_build();
        let endpoint = release(&key, &artifact, artifact.clone());
        let dir = tempfile::tempdir().expect("tempdir");
        let appimage = old_appimage(&dir);
        let mut updater = Updater::new(endpoint, key.public()).with_executable_path(&appimage);

        updater.check_blocking(OLD_VERSION).expect("check");
        updater.snooze();
        assert!(
            updater
                .check_blocking(OLD_VERSION)
                .expect("check")
                .is_none()
        );
        updater.apply_from(OLD_VERSION).expect("apply");

        let expected = format!("ado-planner {OLD_VERSION}");
        assert_eq!(std::fs::read_to_string(&appimage).expect("read"), expected);
    }
}
