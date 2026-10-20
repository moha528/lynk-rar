//! Lecture d'une archive, éventuellement découpée en volumes `nom.7z.001`, `.002`…
//!
//! C'est le découpage de 7-Zip et WinRAR : l'archive est coupée en morceaux bruts,
//! il suffit de les lire bout à bout pour retrouver le fichier d'origine.

use std::{
    fs::{self, File},
    io::{self, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use crate::Result;

/// `x.7z.003` → `Some(("x.7z", 3))`.
fn split_volume(path: &Path) -> Option<(&str, u32)> {
    let name = path.file_name()?.to_str()?;
    let (stem, num) = name.rsplit_once('.')?;
    if num.len() == 3 && num.bytes().all(|b| b.is_ascii_digit()) { Some((stem, num.parse().ok()?)) } else { None }
}

/// Nom de l'archive sans le numéro de volume : `photos.7z.001` → `photos.7z`.
pub(crate) fn logical_name(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    Some(split_volume(path).map_or(name, |(stem, _)| stem.to_owned()))
}

/// Tous les volumes de la série, en partant du premier même si l'on a ouvert le 3ᵉ.
pub fn volumes(path: &Path) -> Vec<PathBuf> {
    let Some((stem, _)) = split_volume(path) else {
        return vec![path.to_path_buf()];
    };
    let dir = path.parent().unwrap_or(Path::new("."));
    (1..1000).map(|n| dir.join(format!("{stem}.{n:03}"))).take_while(|p| p.is_file()).collect()
}

/// Flux lisible et positionnable sur un fichier ou sur une série de volumes.
pub(crate) struct Source {
    parts: Vec<(File, u64)>,
    /// Position de début de chaque volume dans le flux logique.
    starts: Vec<u64>,
    len: u64,
    pos: u64,
}

impl Source {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let mut parts = Vec::new();
        let mut starts = Vec::new();
        let mut len = 0;
        for p in volumes(path) {
            let file = File::open(&p)?;
            let size = file.metadata()?.len();
            starts.push(len);
            len += size;
            parts.push((file, size));
        }
        if parts.is_empty() {
            // Premier volume manquant : on remonte l'erreur d'ouverture du fichier demandé.
            File::open(path)?;
        }
        Ok(Self { parts, starts, len, pos: 0 })
    }

    pub(crate) fn len(&self) -> u64 {
        self.len
    }
}

impl Read for Source {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pos >= self.len || buf.is_empty() {
            return Ok(0);
        }
        let i = self.starts.partition_point(|&s| s <= self.pos) - 1;
        let offset = self.pos - self.starts[i];
        let (file, size) = &mut self.parts[i];
        let want = buf.len().min((*size - offset) as usize);
        file.seek(SeekFrom::Start(offset))?;
        let n = file.read(&mut buf[..want])?;
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for Source {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let target = match pos {
            SeekFrom::Start(p) => p as i128,
            SeekFrom::End(d) => self.len as i128 + d as i128,
            SeekFrom::Current(d) => self.pos as i128 + d as i128,
        };
        if target < 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "position négative"));
        }
        self.pos = target as u64;
        Ok(self.pos)
    }
}

/// Découpe `path` en volumes de `size` octets (`path.001`, `.002`…) et supprime l'original.
/// Rend la liste des volumes ; un fichier déjà assez petit reste tel quel.
pub(crate) fn split_into_volumes(path: &Path, size: u64) -> Result<Vec<PathBuf>> {
    let total = fs::metadata(path)?.len();
    if size == 0 || total <= size {
        return Ok(vec![path.to_path_buf()]);
    }
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut input = File::open(path)?;
    let mut parts = Vec::new();
    let result = (|| {
        let mut n = 1;
        loop {
            let part = dir.join(format!("{name}.{n:03}"));
            let mut out = io::BufWriter::with_capacity(1 << 20, File::create(&part)?);
            parts.push(part);
            let copied = io::copy(&mut (&mut input).take(size), &mut out)?;
            out.flush()?;
            if copied < size {
                break;
            }
            n += 1;
            if n > 999 {
                return Err(crate::Error::Unsupported("plus de 999 volumes : augmentez leur taille"));
            }
        }
        Ok(())
    })();
    drop(input);
    match result {
        Ok(()) => {
            // Volume vide en fin de série quand la taille tombe juste : inutile.
            if let Some(last) = parts.last()
                && parts.len() > 1
                && fs::metadata(last)?.len() == 0
            {
                fs::remove_file(last)?;
                parts.pop();
            }
            fs::remove_file(path)?;
            Ok(parts)
        }
        Err(e) => {
            for p in &parts {
                let _ = fs::remove_file(p);
            }
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_across_volumes() {
        let dir = tempfile::tempdir().unwrap();
        let data: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();
        let file = dir.path().join("a.7z");
        fs::write(&file, &data).unwrap();
        let parts = split_into_volumes(&file, 3000).unwrap();
        assert_eq!(parts.len(), 4);
        assert!(!file.exists());

        // Ouvert depuis le 3ᵉ volume : on relit quand même tout depuis le début.
        let mut src = Source::open(&parts[2]).unwrap();
        assert_eq!(src.len(), 10_000);
        let mut all = Vec::new();
        src.read_to_end(&mut all).unwrap();
        assert_eq!(all, data);

        src.seek(SeekFrom::Start(2990)).unwrap();
        let mut buf = [0u8; 20];
        src.read_exact(&mut buf).unwrap();
        assert_eq!(&buf[..], &data[2990..3010]);
        assert_eq!(logical_name(&parts[1]).as_deref(), Some("a.7z"));
    }
}
