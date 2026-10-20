use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    Zip,
    SevenZ,
    Rar,
    Tar,
    TarGz,
    TarBz2,
    TarXz,
    TarZst,
}

/// Suffixes reconnus, du plus long au plus court pour que `.tar.gz` passe avant `.gz`.
const SUFFIXES: &[(&str, Format)] = &[
    (".tar.gz", Format::TarGz),
    (".tar.bz2", Format::TarBz2),
    (".tar.xz", Format::TarXz),
    (".tar.zst", Format::TarZst),
    (".tgz", Format::TarGz),
    (".tbz2", Format::TarBz2),
    (".tbz", Format::TarBz2),
    (".txz", Format::TarXz),
    (".tzst", Format::TarZst),
    (".tar", Format::Tar),
    (".zip", Format::Zip),
    (".jar", Format::Zip),
    (".7z", Format::SevenZ),
    (".rar", Format::Rar),
];

impl Format {
    pub const ALL: [Format; 8] = [
        Format::Zip,
        Format::SevenZ,
        Format::Rar,
        Format::Tar,
        Format::TarGz,
        Format::TarBz2,
        Format::TarXz,
        Format::TarZst,
    ];

    pub fn extension(self) -> &'static str {
        match self {
            Format::Zip => "zip",
            Format::SevenZ => "7z",
            Format::Rar => "rar",
            Format::Tar => "tar",
            Format::TarGz => "tar.gz",
            Format::TarBz2 => "tar.bz2",
            Format::TarXz => "tar.xz",
            Format::TarZst => "tar.zst",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Zip => "ZIP",
            Format::SevenZ => "7-Zip",
            Format::Rar => "RAR",
            Format::Tar => "TAR",
            Format::TarGz => "TAR.GZ",
            Format::TarBz2 => "TAR.BZ2",
            Format::TarXz => "TAR.XZ",
            Format::TarZst => "TAR.ZST",
        }
    }

    /// RAR est propriétaire : la licence UnRAR autorise l'extraction, pas la création.
    pub fn can_create(self) -> bool {
        self != Format::Rar
    }

    /// Chiffrement possible à la création.
    pub fn can_encrypt(self) -> bool {
        matches!(self, Format::Zip | Format::SevenZ)
    }

    /// Format déduit du nom de fichier (`photos.tar.gz`, `.tgz`, `x.7z`…).
    pub fn from_name(name: &str) -> Option<Format> {
        let lower = name.to_ascii_lowercase();
        SUFFIXES.iter().find(|(suffix, _)| lower.ends_with(suffix)).map(|&(_, f)| f)
    }

    /// Nom sans extension d'archive : `photos.tar.gz` → `photos`, `film.part01.rar` → `film`.
    pub fn strip_extension(name: &str) -> &str {
        // Volume `.001` d'abord : `photos.7z.001` → `photos.7z` → `photos`.
        let name = match name.rsplit_once('.') {
            Some((stem, num)) if num.len() == 3 && num.bytes().all(|b| b.is_ascii_digit()) => stem,
            _ => name,
        };
        let lower = name.to_ascii_lowercase();
        let mut cut = SUFFIXES
            .iter()
            .find(|(suffix, _)| lower.ends_with(suffix) && lower.len() > suffix.len())
            .map_or(name.len(), |(suffix, _)| name.len() - suffix.len());
        if let Some(i) = lower[..cut].rfind(".part") {
            let digits = &lower[i + 5..cut];
            if i > 0 && !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) {
                cut = i;
            }
        }
        &name[..cut]
    }

    /// Détection par signature, puis par extension en repli.
    pub fn detect(path: &Path) -> Result<Format> {
        let mut head = [0u8; 262];
        // Pour une série de volumes, la signature est au début du premier.
        let first = crate::source::volumes(path).into_iter().next().unwrap_or_else(|| path.to_path_buf());
        let n = read_up_to(&mut File::open(&first)?, &mut head)?;
        let by_name = crate::source::logical_name(path).and_then(|n| Format::from_name(&n));
        sniff(&head[..n], by_name).or(by_name).ok_or_else(|| Error::UnknownFormat(path.to_path_buf()))
    }
}

fn sniff(head: &[u8], by_name: Option<Format>) -> Option<Format> {
    let starts = |magic: &[u8]| head.starts_with(magic);
    if starts(b"PK\x03\x04") || starts(b"PK\x05\x06") || starts(b"PK\x07\x08") {
        Some(Format::Zip)
    } else if starts(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) {
        Some(Format::SevenZ)
    } else if starts(b"Rar!\x1A\x07") {
        Some(Format::Rar)
    } else if starts(&[0x1F, 0x8B]) {
        Some(Format::TarGz)
    } else if starts(b"BZh") {
        Some(Format::TarBz2)
    } else if starts(&[0xFD, b'7', b'z', b'X', b'Z', 0x00]) {
        Some(Format::TarXz)
    } else if starts(&[0x28, 0xB5, 0x2F, 0xFD]) {
        Some(Format::TarZst)
    } else if head.get(257..262) == Some(b"ustar") {
        Some(Format::Tar)
    } else if by_name == Some(Format::Tar) && !head.is_empty() {
        // Ancien tar v7 sans signature : on fait confiance à l'extension.
        Some(Format::Tar)
    } else {
        None
    }
}

fn read_up_to(reader: &mut impl Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(Format::from_name("a.TAR.GZ"), Some(Format::TarGz));
        assert_eq!(Format::from_name("a.tgz"), Some(Format::TarGz));
        assert_eq!(Format::from_name("a.gz"), None);
        assert_eq!(Format::strip_extension("photos.tar.gz"), "photos");
        assert_eq!(Format::strip_extension("film.part01.rar"), "film");
        assert_eq!(Format::strip_extension("v1.2.zip"), "v1.2");
        assert_eq!(Format::strip_extension(".zip"), ".zip");
        assert_eq!(Format::strip_extension("photos.7z.001"), "photos");
    }
}
