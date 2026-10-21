//! Aperçu rapide d'un fichier (image, texte) et sommes de contrôle.

use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

use base64::Engine;
use lynk_core::Progress;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, State};

use crate::{
    commands::{CmdError, CmdResult},
    jobs::{JobProgress, Jobs},
};

/// Une image plus lourde ralentirait l'interface pour rien.
const IMAGE_MAX: u64 = 32 << 20;
/// Le début d'un texte suffit à le reconnaître.
const TEXT_MAX: usize = 256 << 10;

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Peek {
    Image {
        url: String,
    },
    Text {
        text: String,
        truncated: bool,
    },
    /// Ni image ni texte, ou trop volumineux.
    Other,
}

fn image_mime(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" | "jfif" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        // Dans un <img>, un SVG n'exécute aucun script.
        "svg" => "image/svg+xml",
        _ => return None,
    })
}

/// Texte lisible si l'échantillon ne contient pas d'octet nul (hors UTF-16 annoncé).
fn decode_text(bytes: &[u8]) -> Option<String> {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).collect();
        return Some(String::from_utf16_lossy(&units));
    }
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    if bytes[..bytes.len().min(8192)].contains(&0) {
        return None;
    }
    Some(String::from_utf8_lossy(bytes).into_owned())
}

fn peek_file(path: &Path) -> io::Result<Peek> {
    let size = std::fs::metadata(path)?.len();
    if let Some(mime) = image_mime(path) {
        if size > IMAGE_MAX {
            return Ok(Peek::Other);
        }
        let b64 = base64::engine::general_purpose::STANDARD.encode(std::fs::read(path)?);
        return Ok(Peek::Image { url: format!("data:{mime};base64,{b64}") });
    }
    let mut buf = Vec::with_capacity(TEXT_MAX.min(size as usize));
    File::open(path)?.take(TEXT_MAX as u64).read_to_end(&mut buf)?;
    Ok(match decode_text(&buf) {
        Some(text) => Peek::Text { text, truncated: size > TEXT_MAX as u64 },
        None => Peek::Other,
    })
}

#[tauri::command]
pub async fn peek(path: String) -> CmdResult<Peek> {
    tauri::async_runtime::spawn_blocking(move || peek_file(Path::new(&path)))
        .await
        .map_err(|e| CmdError::other(e.to_string()))?
        .map_err(|e| CmdError::other(e.to_string()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileHash {
    path: String,
    size: u64,
    crc32: String,
    sha256: String,
}

fn hash_file(path: &Path, progress: &dyn Progress) -> lynk_core::Result<FileHash> {
    progress.entry(&path.file_name().unwrap_or_default().to_string_lossy());
    let mut file = File::open(path)?;
    let mut crc = crc32fast::Hasher::new();
    let mut sha = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    let mut size = 0;
    loop {
        if progress.is_cancelled() {
            return Err(lynk_core::Error::Cancelled);
        }
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        crc.update(&buf[..n]);
        sha.update(&buf[..n]);
        size += n as u64;
        progress.advance(n as u64);
    }
    Ok(FileHash {
        path: path.to_string_lossy().into_owned(),
        size,
        crc32: format!("{:08x}", crc.finalize()),
        sha256: format!("{:x}", sha.finalize()),
    })
}

#[derive(serde::Deserialize)]
pub struct HashRequest {
    job: u32,
    paths: Vec<String>,
}

/// CRC32 et SHA-256 des fichiers (les dossiers sont ignorés), lus une seule fois.
#[tauri::command]
pub async fn hash_files(app: AppHandle, jobs: State<'_, Jobs>, req: HashRequest) -> CmdResult<Vec<FileHash>> {
    let job = req.job;
    let cancel = jobs.start(job);
    let result = tauri::async_runtime::spawn_blocking(move || {
        let progress = JobProgress::new(app, job, cancel);
        let files: Vec<&Path> = req.paths.iter().map(Path::new).filter(|p| p.is_file()).collect();
        progress.total(files.iter().filter_map(|p| p.metadata().ok()).map(|m| m.len()).sum());
        let hashes = files.into_iter().map(|p| hash_file(p, &progress)).collect::<lynk_core::Result<Vec<_>>>();
        progress.flush();
        hashes
    })
    .await
    .map_err(|e| CmdError::other(e.to_string()))?
    .map_err(CmdError::from);
    jobs.finish(job);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_detection() {
        assert_eq!(decode_text(b"\xEF\xBB\xBFbonjour").as_deref(), Some("bonjour"));
        assert_eq!(decode_text(&[0xFF, 0xFE, b'h', 0, b'i', 0]).as_deref(), Some("hi"));
        assert_eq!(decode_text(b"PK\x03\x04\0\0"), None);
        assert_eq!(decode_text("été".as_bytes()).as_deref(), Some("été"));
    }

    #[test]
    fn known_hashes() {
        let dir = std::env::temp_dir().join(format!("lynk-hash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("abc.txt");
        std::fs::write(&file, "abc").unwrap();
        let h = hash_file(&file, &lynk_core::NoProgress).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(h.crc32, "352441c2");
        assert_eq!(h.sha256, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(h.size, 3);
    }
}
