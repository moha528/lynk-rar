use std::{
    fs::{self, File},
    io::{self, BufReader, BufWriter, Write},
    path::Path,
    time::SystemTime,
};

use zip::{AesMode, CompressionMethod, ZipArchive, ZipWriter, result::ZipError, write::SimpleFileOptions};

use crate::{
    CreateOptions, Entry, Error, ExtractOptions, ExtractReport, Progress, Result, UpdateOptions,
    paths::{self, Item},
    progress::{Tracked, check},
    source::Source,
    time,
};

fn open(path: &Path) -> Result<ZipArchive<BufReader<Source>>> {
    ZipArchive::new(BufReader::new(Source::open(path)?)).map_err(map_err)
}

fn map_err(e: ZipError) -> Error {
    match e {
        ZipError::Io(e) => e.into(),
        ZipError::InvalidPassword => Error::Password,
        ZipError::UnsupportedArchive(m) if m.to_ascii_lowercase().contains("password") => Error::Password,
        e => Error::Archive(e.to_string()),
    }
}

fn to_unix(dt: zip::DateTime) -> Option<i64> {
    time::local_to_unix(
        dt.year().into(),
        dt.month().into(),
        dt.day().into(),
        dt.hour().into(),
        dt.minute().into(),
        dt.second().into(),
    )
}

fn to_zip_time(t: SystemTime) -> Option<zip::DateTime> {
    let p = time::local_parts(t);
    zip::DateTime::from_date_and_time(
        p.year.try_into().ok()?,
        p.month as u8,
        p.day as u8,
        p.hour as u8,
        p.minute as u8,
        p.second as u8,
    )
    .ok()
}

pub(super) fn list(path: &Path) -> Result<Vec<Entry>> {
    let mut archive = open(path)?;
    let mut out = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let f = archive.by_index_raw(i).map_err(map_err)?;
        let mut e = Entry::new(&f.name().map_err(map_err)?, f.is_dir(), f.size());
        e.packed_size = Some(f.compressed_size());
        e.modified = f.last_modified().and_then(to_unix);
        e.encrypted = f.encrypted();
        out.push(e);
    }
    Ok(out)
}

pub(super) fn extract(
    path: &Path,
    dest: &Path,
    opts: &ExtractOptions,
    progress: &dyn Progress,
) -> Result<ExtractReport> {
    let mut archive = open(path)?;
    let mut selected = Vec::with_capacity(archive.len());
    let mut total = 0;
    for i in 0..archive.len() {
        let f = archive.by_index_raw(i).map_err(map_err)?;
        let name = f.name().map_err(map_err)?.into_owned();
        if let Some(effective) = opts.effective_name(&name) {
            total += f.size();
            selected.push((i, name, effective));
        }
    }
    progress.total(total);

    let mut report = ExtractReport::default();
    for (i, name, effective) in selected {
        check(progress)?;
        let mut f = match opts.password.as_deref() {
            Some(pw) => archive.by_index_decrypt(i, pw.as_bytes()),
            None => archive.by_index(i),
        }
        .map_err(map_err)?;
        progress.entry(&name);
        let Some(rel) = paths::safe_relative(&effective) else {
            report.skipped.push(name);
            continue;
        };
        let target = dest.join(rel);
        if f.is_dir() {
            fs::create_dir_all(&target)?;
            report.dirs += 1;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let Some(target) = paths::resolve_target(target, opts.overwrite) else {
            progress.advance(f.size());
            report.skipped.push(name);
            continue;
        };
        let modified = f.last_modified().and_then(to_unix).map(time::from_unix);
        report.bytes += super::write_file(&target, &mut Tracked::new(&mut f, progress), modified)?;
        report.files += 1;
    }
    Ok(report)
}

pub(super) fn create(items: &[Item], dest: &Path, opts: &CreateOptions, progress: &dyn Progress) -> Result<()> {
    progress.total(items.iter().map(|it| it.size).sum());
    let mut zip = ZipWriter::new(BufWriter::with_capacity(1 << 20, File::create(dest)?));
    write_items(&mut zip, items, opts.level, opts.password.as_deref(), progress)?;
    zip.finish().map_err(map_err)?.flush()?;
    Ok(())
}

fn write_items<W: Write + io::Seek>(
    zip: &mut ZipWriter<W>,
    items: &[Item],
    level: u32,
    password: Option<&str>,
    progress: &dyn Progress,
) -> Result<()> {
    let (method, level) = match level {
        0 => (CompressionMethod::Stored, None),
        l => (CompressionMethod::Deflated, Some(l.min(9).into())),
    };
    let base = SimpleFileOptions::default().compression_method(method).compression_level(level);

    for it in items {
        check(progress)?;
        progress.entry(&it.name);
        let mut options = base.large_file(it.size >= u64::from(u32::MAX));
        if let Some(t) = it.modified.and_then(to_zip_time) {
            options = options.last_modified_time(t);
        }
        if it.is_dir {
            zip.add_directory(it.name.as_str(), options).map_err(map_err)?;
            continue;
        }
        match password {
            Some(pw) => zip.start_file(it.name.as_str(), options.with_aes_encryption(AesMode::Aes256, pw)),
            None => zip.start_file(it.name.as_str(), options),
        }
        .map_err(map_err)?;
        io::copy(&mut Tracked::new(File::open(&it.path)?, progress), zip)?;
    }
    Ok(())
}

pub(super) fn update(
    archive: &Path,
    tmp: &Path,
    items: &[Item],
    doomed: &dyn Fn(&str) -> bool,
    opts: &UpdateOptions,
    progress: &dyn Progress,
) -> Result<()> {
    let mut src = open(archive)?;
    let mut keep = Vec::with_capacity(src.len());
    let mut total: u64 = items.iter().map(|it| it.size).sum();
    for i in 0..src.len() {
        let f = src.by_index_raw(i).map_err(map_err)?;
        if !doomed(&f.name().map_err(map_err)?) {
            total += f.compressed_size();
            keep.push(i);
        }
    }
    progress.total(total);

    let mut zip = ZipWriter::new(BufWriter::with_capacity(1 << 20, File::create(tmp)?));
    for i in keep {
        check(progress)?;
        let f = src.by_index_raw(i).map_err(map_err)?;
        let packed = f.compressed_size();
        progress.entry(&f.name().map_err(map_err)?);
        // Données compressées (et chiffrées) recopiées telles quelles.
        zip.raw_copy_file(f).map_err(map_err)?;
        progress.advance(packed);
    }
    write_items(&mut zip, items, opts.level, opts.password.as_deref(), progress)?;
    zip.finish().map_err(map_err)?.flush()?;
    Ok(())
}

pub(super) fn test(path: &Path, password: Option<&str>, progress: &dyn Progress) -> Result<crate::TestReport> {
    let mut archive = open(path)?;
    let mut total = 0;
    for i in 0..archive.len() {
        total += archive.by_index_raw(i).map_err(map_err)?.size();
    }
    progress.total(total);
    let mut report = crate::TestReport::default();
    for i in 0..archive.len() {
        check(progress)?;
        let mut f = match password {
            Some(pw) => archive.by_index_decrypt(i, pw.as_bytes()),
            None => archive.by_index(i),
        }
        .map_err(map_err)?;
        if f.is_dir() {
            continue;
        }
        let name = f.name().map_err(map_err)?.into_owned();
        progress.entry(&name);
        // Lire jusqu'au bout déclenche la vérification du CRC-32.
        match io::copy(&mut Tracked::new(&mut f, progress), &mut io::sink()) {
            Ok(n) => {
                report.files += 1;
                report.bytes += n;
            }
            Err(e) => {
                check(progress)?;
                report.errors.push(format!("{name} : {e}"));
            }
        }
    }
    Ok(report)
}
