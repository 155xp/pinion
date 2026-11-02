//! Downloads NVIDIA's Parakeet v3 (4-bit, 416 MB) once, verifies it, and keeps it on disk.

use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub const NAME: &str = "Parakeet v3";
pub const FILE: &str = "parakeet-tdt-0.6b-v3-q4_k.bin";
pub const SIZE: u64 = 415_611_879;
const SHA256: &str = "8b205b8b39c6535e153de6fb11c51db46125d45c4f16ba496fe41a0fe71b885e";
const URL: &str = "https://huggingface.co/ggml-org/parakeet-GGUF/resolve/35156454d1a39de06863303dd209fd2bed6ee079/ggml-parakeet-tdt-0.6b-v3-q4_k.bin";

pub fn path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

pub fn is_ready(dir: &Path) -> bool {
    fs::metadata(path(dir)).map(|m| m.len() == SIZE).unwrap_or(false)
}

/// Downloads with resume support. `progress` gets bytes so far and the total.
pub fn download(dir: &Path, cancel: &AtomicBool, mut progress: impl FnMut(u64, u64)) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let target = path(dir);
    let part = dir.join(format!("{FILE}.part"));
    let mut have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if have > SIZE {
        have = 0;
    }

    let tls = ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .tls_config(tls)
        .user_agent(concat!("Pinion/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent();
    let mut req = agent.get(URL);
    if have > 0 && have < SIZE {
        req = req.header("Range", &format!("bytes={have}-"));
    }
    let mut res = req.call().map_err(|e| format!("Couldn't reach the download: {e}"))?;
    if res.status() == 200 {
        have = 0;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(have > 0)
        .truncate(have == 0)
        .open(&part)
        .map_err(|e| e.to_string())?;
    if have < SIZE {
        let mut reader = res.body_mut().with_config().limit(SIZE + 1).reader();
        let mut buf = vec![0u8; 256 * 1024];
        let mut last = 0u64;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("cancelled".into());
            }
            let n = reader.read(&mut buf).map_err(|e| format!("The download stopped: {e}"))?;
            if n == 0 {
                break;
            }
            file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            have += n as u64;
            if have - last > 1_000_000 || have == SIZE {
                progress(have, SIZE);
                last = have;
            }
        }
    }
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    if have != SIZE {
        return Err("The download ended early. Try again to pick up where it stopped.".into());
    }
    if sha256(&part)? != SHA256 {
        let _ = fs::remove_file(&part);
        return Err("The download was damaged. Try again.".into());
    }
    fs::rename(&part, &target).map_err(|e| e.to_string())?;
    progress(SIZE, SIZE);
    Ok(())
}

fn sha256(p: &Path) -> Result<String, String> {
    let mut f = fs::File::open(p).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

pub fn remove(dir: &Path) {
    let _ = fs::remove_file(path(dir));
    let _ = fs::remove_file(dir.join(format!("{FILE}.part")));
}
