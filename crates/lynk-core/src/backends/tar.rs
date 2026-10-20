use std::{
    fs::{self, File},
    io::{BufReader, BufWriter, Read, Write},
    path::Path,
};

use tar::EntryType;

use crate::{
    CreateOptions, Entry, ExtractOptions, ExtractReport, Format, NoProgress, Progress, Result,
    paths::{self, Item},
    progress::{Tracked, check},
    source::Source,
    time,
};

/// Flux tar décompressé. L'avancement est mesuré sur les octets compressés lus
/// sur le disque : c'est la seule taille connue d'avance pour un flux.
fn reader<'a>(format: Format, path: &Path, progress: &'a dyn Progress) -> Result<Box<dyn Read + 'a>> {
    let file = BufReader::with_capacity(1 << 20, Tracked::new(Source::open(path)?, progress));
    Ok(match format {
        Format::TarGz => Box::new(flate2::bufread::MultiGzDecoder::new(file)),
        Format::TarBz2 => Box::new(bzip2::bufread::MultiBzDecoder::new(file)),
        Format::TarXz => Box::new(xz2::bufread::XzDecoder::new_multi_decoder(file)),
        Format::TarZst => Box::new(zstd::stream::read::Decoder::with_buffer(file)?),
        _ => Box::new(file),
    })
}

fn listed(kind: EntryType) -> bool {
    kind.is_file() || kind.is_dir() || kind == EntryType::Continuous
}

pub(super) fn list(format: Format, path: &Path) -> Result<Vec<Entry>> {
    let mut archive = tar::Archive::new(reader(format, path, &NoProgress)?);
    let mut out = Vec::new();
    for e in archive.entries()? {
        let e = e?;
        let kind = e.header().entry_type();
        if !listed(kind) {
            continue;
        }
        let mut entry = Entry::new(&e.path()?.to_string_lossy(), kind.is_dir(), e.size());
        entry.modified = e.header().mtime().ok().map(|t| t as i64);
        out.push(entry);
    }
    Ok(out)
}

pub(super) fn extract(
    format: Format,
    path: &Path,
    dest: &Path,
    opts: &ExtractOptions,
    progress: &dyn Progress,
) -> Result<ExtractReport> {
    progress.total(Source::open(path)?.len());
    let mut archive = tar::Archive::new(reader(format, path, progress)?);
    let mut report = ExtractReport::default();
    for e in archive.entries()? {
        check(progress)?;
        let mut e = e?;
        let name = e.path()?.to_string_lossy().into_owned();
        let Some(effective) = opts.effective_name(&name) else {
            continue;
        };
        progress.entry(&name);
        let kind = e.header().entry_type();
        let Some(rel) = paths::safe_relative(&effective) else {
            report.skipped.push(name);
            continue;
        };
        let target = dest.join(rel);
        if kind.is_dir() {
            fs::create_dir_all(&target)?;
            report.dirs += 1;
            continue;
        }
        // Liens symboliques et physiques : ignorés, ils pourraient pointer hors de la destination.
        if !listed(kind) {
            report.skipped.push(name);
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let Some(target) = paths::resolve_target(target, opts.overwrite) else {
            report.skipped.push(name);
            continue;
        };
        let modified = e.header().mtime().ok().map(|t| time::from_unix(t as i64));
        report.bytes += super::write_file(&target, &mut e, modified)?;
        report.files += 1;
    }
    Ok(report)
}

pub(super) fn create(
    format: Format,
    items: &[Item],
    dest: &Path,
    opts: &CreateOptions,
    progress: &dyn Progress,
) -> Result<()> {
    progress.total(items.iter().map(|it| it.size).sum());
    let file = BufWriter::with_capacity(1 << 20, File::create(dest)?);
    let level = opts.level.min(9);
    match format {
        Format::TarGz => {
            let enc = flate2::write::GzEncoder::new(file, flate2::Compression::new(level));
            write(enc, items, progress)?.finish()?.flush()?;
        }
        Format::TarBz2 => {
            let enc = bzip2::write::BzEncoder::new(file, bzip2::Compression::new(level.max(1)));
            write(enc, items, progress)?.finish()?.flush()?;
        }
        Format::TarXz => {
            let enc = xz2::write::XzEncoder::new(file, level);
            write(enc, items, progress)?.finish()?.flush()?;
        }
        Format::TarZst => {
            const ZSTD: [i32; 10] = [1, 1, 2, 3, 5, 7, 9, 12, 15, 19];
            let enc = zstd::Encoder::new(file, ZSTD[level as usize])?;
            write(enc, items, progress)?.finish()?.flush()?;
        }
        _ => write(file, items, progress)?.flush()?,
    }
    Ok(())
}

fn write<W: Write>(w: W, items: &[Item], progress: &dyn Progress) -> Result<W> {
    let mut builder = tar::Builder::new(w);
    for it in items {
        check(progress)?;
        progress.entry(&it.name);
        if it.is_dir {
            builder.append_dir(&it.name, &it.path)?;
            continue;
        }
        let file = File::open(&it.path)?;
        let mut header = tar::Header::new_gnu();
        header.set_metadata(&file.metadata()?);
        builder.append_data(&mut header, &it.name, Tracked::new(file, progress))?;
    }
    Ok(builder.into_inner()?)
}

pub(super) fn test(format: Format, path: &Path, progress: &dyn Progress) -> Result<crate::TestReport> {
    progress.total(Source::open(path)?.len());
    let mut archive = tar::Archive::new(reader(format, path, progress)?);
    let mut report = crate::TestReport::default();
    // Un flux corrompu ne se relit pas au-delà de l'erreur : on s'arrête à la première.
    'entries: {
        let entries = match archive.entries() {
            Ok(entries) => entries,
            Err(e) => {
                report.errors.push(e.to_string());
                break 'entries;
            }
        };
        for e in entries {
            check(progress)?;
            let mut e = match e {
                Ok(e) => e,
                Err(err) => {
                    report.errors.push(err.to_string());
                    break 'entries;
                }
            };
            if !listed(e.header().entry_type()) || e.header().entry_type().is_dir() {
                continue;
            }
            let name = e.path().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
            progress.entry(&name);
            match std::io::copy(&mut e, &mut std::io::sink()) {
                Ok(n) => {
                    report.files += 1;
                    report.bytes += n;
                }
                Err(err) => {
                    check(progress)?;
                    report.errors.push(format!("{name} : {err}"));
                    break 'entries;
                }
            }
        }
    }
    // Le CRC de gzip ou de bzip2 est en fin de flux, après le dernier bloc tar : on va le lire.
    if report.errors.is_empty()
        && let Err(e) = std::io::copy(&mut archive.into_inner(), &mut std::io::sink())
    {
        check(progress)?;
        report.errors.push(e.to_string());
    }
    Ok(report)
}
