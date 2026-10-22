use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};

use winsar_core::{CreateOptions, Error, ExtractOptions, Format, Overwrite, Progress, Selection, UpdateOptions};

fn sample(root: &Path) -> PathBuf {
    let src = root.join("projet");
    fs::create_dir_all(src.join("docs/vide")).unwrap();
    fs::write(src.join("lisez-moi.txt"), "bonjour l'archive\n".repeat(200)).unwrap();
    fs::write(src.join("docs/données é.bin"), (0..50_000u32).map(|i| (i * 7) as u8).collect::<Vec<_>>()).unwrap();
    fs::write(src.join("docs/zéro.txt"), "").unwrap();
    src
}

fn files_under(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out: Vec<_> = walkdir(dir)
        .into_iter()
        .map(|p| {
            let rel = p.strip_prefix(dir).unwrap().to_string_lossy().replace('\\', "/");
            (rel, fs::read(&p).unwrap())
        })
        .collect();
    out.sort();
    out
}

fn walkdir(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(walkdir(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[derive(Default)]
struct Counter {
    bytes: AtomicU64,
    cancel: AtomicBool,
}

impl Progress for Counter {
    fn advance(&self, n: u64) {
        self.bytes.fetch_add(n, Ordering::Relaxed);
    }
    fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

fn roundtrip(format: Format, password: Option<&str>) {
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path());
    let archive = tmp.path().join(format!("sortie.{}", format.extension()));
    let opts = CreateOptions { format, level: 6, password: password.map(str::to_owned), ..Default::default() };
    let counter = Counter::default();
    winsar_core::create(std::slice::from_ref(&src), &archive, &opts, &counter).unwrap();
    assert!(counter.bytes.load(Ordering::Relaxed) > 0, "{format:?} : aucun avancement");
    assert_eq!(Format::detect(&archive).unwrap(), format);

    let entries = winsar_core::list(&archive, password).unwrap();
    let names: Vec<_> = entries.iter().map(|e| e.path.as_str()).collect();
    assert!(names.contains(&"projet/docs/données é.bin"), "{format:?} : {names:?}");
    assert!(entries.iter().any(|e| e.is_dir && e.path == "projet/docs/vide"), "{format:?} : {names:?}");
    let files = entries.iter().filter(|e| !e.is_dir && e.size > 0);
    assert!(files.clone().all(|e| e.encrypted == password.is_some()), "{format:?} : chiffrement mal signalé");

    // Une seule racine « projet » : l'extraction intelligente dépose à côté de l'archive.
    let out = tmp.path().join("extrait");
    fs::create_dir(&out).unwrap();
    let moved = out.join(archive.file_name().unwrap());
    fs::rename(&archive, &moved).unwrap();
    let report = winsar_core::extract_smart(
        &moved,
        &ExtractOptions { password: password.map(str::to_owned), overwrite: Overwrite::Rename, ..Default::default() },
        &winsar_core::NoProgress,
    )
    .unwrap();
    assert_eq!(report.destination, out);
    assert_eq!(files_under(&out.join("projet")), files_under(&src), "{format:?}");
    assert!(out.join("projet/docs/vide").is_dir());

    // Deuxième extraction : la racine existe déjà, on crée un dossier au nom de l'archive.
    let again = winsar_core::extract_smart(
        &moved,
        &ExtractOptions { password: password.map(str::to_owned), ..Default::default() },
        &winsar_core::NoProgress,
    )
    .unwrap();
    assert_eq!(again.destination, out.join("sortie"));
}

#[test]
fn zip() {
    roundtrip(Format::Zip, None);
}

#[test]
fn zip_aes() {
    roundtrip(Format::Zip, Some("s3cret"));
}

#[test]
fn sevenz() {
    roundtrip(Format::SevenZ, None);
}

#[test]
fn sevenz_aes() {
    roundtrip(Format::SevenZ, Some("s3cret"));
}

#[test]
fn tar_family() {
    for f in [Format::Tar, Format::TarGz, Format::TarBz2, Format::TarXz, Format::TarZst] {
        roundtrip(f, None);
    }
}

#[test]
fn partial_extraction_is_relative_to_current_folder() {
    for format in [Format::Zip, Format::SevenZ, Format::TarGz] {
        let tmp = tempfile::tempdir().unwrap();
        let src = sample(tmp.path());
        let archive = tmp.path().join(format!("a.{}", format.extension()));
        winsar_core::create(
            &[src],
            &archive,
            &CreateOptions { format, ..Default::default() },
            &winsar_core::NoProgress,
        )
        .unwrap();
        let out = tmp.path().join("out");
        let opts = ExtractOptions {
            selection: Some(Selection {
                base: "projet/docs".into(),
                items: vec!["projet/docs/zéro.txt".into(), "projet/docs/vide".into()],
            }),
            ..Default::default()
        };
        let report = winsar_core::extract(&archive, &out, &opts, &winsar_core::NoProgress).unwrap();
        assert_eq!((report.files, report.dirs), (1, 1), "{format:?}");
        assert!(out.join("zéro.txt").is_file() && out.join("vide").is_dir(), "{format:?}");
        assert!(!out.join("projet").exists() && !out.join("données é.bin").exists(), "{format:?}");
    }
}

#[test]
fn update_adds_replaces_and_removes() {
    let cases =
        [(Format::Zip, None), (Format::Zip, Some("s3cret")), (Format::SevenZ, Some("s3cret")), (Format::TarGz, None)];
    for (format, pw) in cases {
        let tmp = tempfile::tempdir().unwrap();
        let src = sample(tmp.path());
        let archive = tmp.path().join(format!("a.{}", format.extension()));
        let password = pw.map(str::to_owned);
        winsar_core::create(
            std::slice::from_ref(&src),
            &archive,
            &CreateOptions { format, level: 6, password: password.clone(), ..Default::default() },
            &winsar_core::NoProgress,
        )
        .unwrap();

        let extra = tmp.path().join("nouveau.txt");
        fs::write(&extra, "ajouté").unwrap();
        let opts = UpdateOptions { password: password.clone(), level: 6 };
        let remove = ["projet/lisez-moi.txt".to_owned()];
        winsar_core::update(
            &archive,
            "projet/docs",
            std::slice::from_ref(&extra),
            &remove,
            &opts,
            &winsar_core::NoProgress,
        )
        .unwrap();

        // Deuxième ajout du même nom : remplacement, pas de doublon.
        fs::write(&extra, "v2").unwrap();
        winsar_core::update(
            &archive,
            "projet/docs",
            std::slice::from_ref(&extra),
            &[],
            &opts,
            &winsar_core::NoProgress,
        )
        .unwrap();

        let entries = winsar_core::list(&archive, pw).unwrap();
        let count = |p: &str| entries.iter().filter(|e| e.path == p).count();
        assert_eq!(count("projet/docs/nouveau.txt"), 1, "{format:?}");
        assert_eq!(count("projet/lisez-moi.txt"), 0, "{format:?}");
        assert_eq!(count("projet/docs/données é.bin"), 1, "{format:?}");
        if pw.is_some() {
            assert!(entries.iter().filter(|e| !e.is_dir).all(|e| e.encrypted), "{format:?} : chiffrement perdu");
        }

        let out = tmp.path().join("out");
        let extract = ExtractOptions { password, ..Default::default() };
        winsar_core::extract(&archive, &out, &extract, &winsar_core::NoProgress).unwrap();
        assert_eq!(fs::read_to_string(out.join("projet/docs/nouveau.txt")).unwrap(), "v2", "{format:?}");
        assert_eq!(
            fs::read(out.join("projet/docs/données é.bin")).unwrap(),
            fs::read(src.join("docs/données é.bin")).unwrap(),
            "{format:?}"
        );
        let leftovers: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with('.'))
            .collect();
        assert!(leftovers.is_empty(), "{format:?} : fichiers temporaires {leftovers:?}");
    }
}

#[test]
fn volumes_roundtrip() {
    for format in [Format::Zip, Format::SevenZ, Format::TarXz] {
        let tmp = tempfile::tempdir().unwrap();
        let src = sample(tmp.path());
        // Données peu compressibles pour obtenir plusieurs volumes.
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        let noise: Vec<u8> = (0..200_000)
            .map(|_| {
                // xorshift64 : pseudo-aléatoire, donc incompressible.
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 24) as u8
            })
            .collect();
        fs::write(src.join("bruit.bin"), &noise).unwrap();
        let archive = tmp.path().join(format!("v.{}", format.extension()));
        let opts = CreateOptions { format, level: 1, volume_size: Some(64 * 1024), ..Default::default() };
        let parts = winsar_core::create(std::slice::from_ref(&src), &archive, &opts, &winsar_core::NoProgress).unwrap();
        assert!(parts.len() >= 3, "{format:?} : {} volumes", parts.len());
        assert!(!archive.exists());
        assert!(parts[0].to_string_lossy().ends_with(".001"));
        assert_eq!(winsar_core::volumes(&parts[1]), parts);

        // Ouverture depuis un volume du milieu, comme un double-clic au hasard.
        assert_eq!(Format::detect(&parts[1]).unwrap(), format);
        assert!(winsar_core::test(&parts[1], None, &winsar_core::NoProgress).unwrap().is_ok(), "{format:?}");
        let report =
            winsar_core::extract_smart(&parts[0], &ExtractOptions::default(), &winsar_core::NoProgress).unwrap();
        // « projet » existe déjà à côté (la source) : dossier au nom logique « v », sans « .zip.001 ».
        assert_eq!(report.destination, tmp.path().join("v"), "{format:?}");
        assert_eq!(files_under(&tmp.path().join("v/projet")), files_under(&src), "{format:?}");
        assert!(
            winsar_core::update(
                &parts[0],
                "",
                &[],
                &["projet/bruit.bin".into()],
                &Default::default(),
                &winsar_core::NoProgress
            )
            .is_err()
        );
    }
}

#[test]
fn integrity_test_detects_corruption() {
    for format in [Format::Zip, Format::SevenZ, Format::TarGz] {
        let tmp = tempfile::tempdir().unwrap();
        let src = sample(tmp.path());
        let archive = tmp.path().join(format!("c.{}", format.extension()));
        winsar_core::create(
            &[src],
            &archive,
            &CreateOptions { format, ..Default::default() },
            &winsar_core::NoProgress,
        )
        .unwrap();
        let sane = winsar_core::test(&archive, None, &winsar_core::NoProgress).unwrap();
        assert!(sane.is_ok() && sane.files >= 2, "{format:?} : {sane:?}");

        let mut bytes = fs::read(&archive).unwrap();
        let mid = bytes.len() / 2;
        for b in &mut bytes[mid..mid + 32] {
            *b ^= 0x5A;
        }
        fs::write(&archive, bytes).unwrap();
        let broken = winsar_core::test(&archive, None, &winsar_core::NoProgress);
        assert!(!matches!(&broken, Ok(r) if r.is_ok()), "{format:?} : corruption non détectée");
    }
}

#[test]
fn wrong_password_is_reported() {
    for format in [Format::Zip, Format::SevenZ] {
        let tmp = tempfile::tempdir().unwrap();
        let src = sample(tmp.path());
        let archive = tmp.path().join(format!("a.{}", format.extension()));
        let opts = CreateOptions { format, level: 1, password: Some("bon".into()), ..Default::default() };
        winsar_core::create(&[src], &archive, &opts, &winsar_core::NoProgress).unwrap();
        let bad = ExtractOptions { password: Some("faux".into()), ..Default::default() };
        let err = winsar_core::extract(&archive, &tmp.path().join("x"), &bad, &winsar_core::NoProgress).unwrap_err();
        assert!(matches!(err, Error::Password), "{format:?} : {err}");
        let none =
            winsar_core::extract(&archive, &tmp.path().join("y"), &ExtractOptions::default(), &winsar_core::NoProgress)
                .unwrap_err();
        assert!(matches!(none, Error::Password), "{format:?} sans mot de passe : {none}");
    }
}

#[test]
fn cancel_removes_partial_archive() {
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path());
    let archive = tmp.path().join("annule.zip");
    let counter = Counter::default();
    counter.cancel.store(true, Ordering::Relaxed);
    let err = winsar_core::create(&[src], &archive, &CreateOptions::default(), &counter).unwrap_err();
    assert!(matches!(err, Error::Cancelled));
    assert!(!archive.exists());
}

#[test]
fn rar_is_read_only() {
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path());
    let opts = CreateOptions { format: Format::Rar, ..Default::default() };
    let err = winsar_core::create(&[src], &tmp.path().join("a.rar"), &opts, &winsar_core::NoProgress).unwrap_err();
    assert!(matches!(err, Error::CannotCreate(Format::Rar)));
}

#[test]
fn default_names() {
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path());
    let file = src.join("lisez-moi.txt");
    assert_eq!(winsar_core::default_archive_path(std::slice::from_ref(&file), Format::Zip), src.join("lisez-moi.zip"));
    fs::write(src.join("lisez-moi.zip"), "").unwrap();
    assert_eq!(winsar_core::default_archive_path(&[file], Format::Zip), src.join("lisez-moi (2).zip"));
    let tgz = src.join("photos.tar.gz");
    fs::write(&tgz, "").unwrap();
    assert_eq!(winsar_core::default_archive_path(&[tgz], Format::Zip), src.join("photos.zip"));
    let volume = src.join("film.7z.001");
    fs::write(&volume, "").unwrap();
    assert_eq!(winsar_core::default_archive_path(&[volume], Format::Zip), src.join("film.zip"));
    assert_eq!(
        winsar_core::default_archive_path(&[src.join("docs"), src.join("lisez-moi.txt")], Format::SevenZ),
        src.join("projet.7z")
    );
}

#[derive(Default)]
struct Bar {
    total: AtomicU64,
    done: AtomicU64,
    totals: AtomicU64,
}

impl Progress for Bar {
    fn total(&self, n: u64) {
        self.total.store(n, Ordering::Relaxed);
        self.totals.fetch_add(1, Ordering::Relaxed);
    }
    fn advance(&self, n: u64) {
        self.done.fetch_add(n, Ordering::Relaxed);
    }
}

#[test]
fn convert_keeps_content() {
    let cases = [
        (Format::Zip, None, Format::SevenZ, Some("s3cret")),
        (Format::SevenZ, Some("s3cret"), Format::TarZst, None),
        (Format::TarGz, None, Format::Zip, None),
    ];
    for (from, from_pw, to, to_pw) in cases {
        let tmp = tempfile::tempdir().unwrap();
        let src = sample(tmp.path());
        let archive = tmp.path().join(format!("a.{}", from.extension()));
        let opts = CreateOptions { format: from, password: from_pw.map(str::to_owned), ..Default::default() };
        winsar_core::create(std::slice::from_ref(&src), &archive, &opts, &winsar_core::NoProgress).unwrap();

        let dest = winsar_core::default_archive_path(std::slice::from_ref(&archive), to);
        assert_eq!(dest, tmp.path().join(format!("a.{}", to.extension())), "{from:?} → {to:?}");
        let convert = winsar_core::ConvertOptions {
            password: from_pw.map(str::to_owned),
            create: CreateOptions { format: to, level: 3, password: to_pw.map(str::to_owned), volume_size: None },
        };
        let bar = Bar::default();
        let parts = winsar_core::convert(&archive, &dest, &convert, &bar).unwrap();
        assert_eq!(parts, std::slice::from_ref(&dest));
        // Une seule barre pour les deux étapes, remplie exactement.
        assert_eq!(bar.totals.load(Ordering::Relaxed), 1, "{from:?} → {to:?}");
        assert_eq!(bar.done.load(Ordering::Relaxed), bar.total.load(Ordering::Relaxed), "{from:?} → {to:?}");

        assert_eq!(Format::detect(&dest).unwrap(), to);
        let out = tmp.path().join("out");
        let extract = ExtractOptions { password: to_pw.map(str::to_owned), ..Default::default() };
        winsar_core::extract(&dest, &out, &extract, &winsar_core::NoProgress).unwrap();
        assert_eq!(files_under(&out.join("projet")), files_under(&src), "{from:?} → {to:?}");
        assert!(out.join("projet/docs/vide").is_dir(), "{from:?} → {to:?}");
        let leftovers: Vec<_> = fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with('.'))
            .collect();
        assert!(leftovers.is_empty(), "{from:?} → {to:?} : fichiers temporaires {leftovers:?}");
    }
}

#[test]
fn convert_refuses_to_overwrite_source() {
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path());
    let archive = tmp.path().join("a.zip");
    winsar_core::create(&[src], &archive, &CreateOptions::default(), &winsar_core::NoProgress).unwrap();
    let before = fs::read(&archive).unwrap();
    let opts = winsar_core::ConvertOptions::default();
    assert!(winsar_core::convert(&archive, &archive, &opts, &winsar_core::NoProgress).is_err());
    assert_eq!(fs::read(&archive).unwrap(), before);
}
