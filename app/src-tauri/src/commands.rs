use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU32, Ordering},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use winsar_core::{CreateOptions, Entry, ExtractOptions, ExtractReport, Format, Overwrite, Selection};

use crate::{
    jobs::{JobProgress, Jobs},
    launch::Launch,
};

/// Erreur telle que l'interface la reçoit : `kind` permet de réagir
/// (redemander le mot de passe, ignorer une annulation) sans analyser le message.
#[derive(Debug, Serialize)]
pub struct CmdError {
    kind: &'static str,
    message: String,
}

impl CmdError {
    pub(crate) fn other(message: impl Into<String>) -> Self {
        Self { kind: "error", message: message.into() }
    }
}

impl From<winsar_core::Error> for CmdError {
    fn from(e: winsar_core::Error) -> Self {
        let kind = match e {
            winsar_core::Error::Password => "password",
            winsar_core::Error::Cancelled => "cancelled",
            _ => "error",
        };
        Self { kind, message: e.to_string() }
    }
}

pub type CmdResult<T> = Result<T, CmdError>;

/// Le moteur est synchrone : chaque opération part sur un thread dédié.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> winsar_core::Result<T> + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| CmdError::other(e.to_string()))?.map_err(Into::into)
}

#[tauri::command]
pub fn launch_info(launch: State<'_, Launch>) -> Launch {
    launch.inner().clone()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FormatInfo {
    id: Format,
    label: &'static str,
    extension: &'static str,
    can_create: bool,
    can_encrypt: bool,
}

#[tauri::command]
pub fn formats() -> Vec<FormatInfo> {
    Format::ALL
        .into_iter()
        .map(|f| FormatInfo {
            id: f,
            label: f.label(),
            extension: f.extension(),
            can_create: f.can_create(),
            can_encrypt: f.can_encrypt(),
        })
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveInfo {
    path: String,
    name: String,
    format: Format,
    format_label: &'static str,
    /// Taille sur le disque, tous volumes compris.
    size: u64,
    /// Nombre de volumes `.001`, `.002`… (1 pour une archive d'un seul tenant).
    volumes: usize,
    entries: Vec<Entry>,
}

#[tauri::command]
pub async fn open_archive(path: String, password: Option<String>) -> CmdResult<ArchiveInfo> {
    blocking(move || {
        let p = PathBuf::from(&path);
        let format = Format::detect(&p)?;
        let entries = winsar_core::list(&p, password.as_deref())?;
        let volumes = winsar_core::volumes(&p);
        let mut size = 0;
        for v in &volumes {
            size += std::fs::metadata(v)?.len();
        }
        Ok(ArchiveInfo {
            name: p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
            size,
            volumes: volumes.len().max(1),
            format,
            format_label: format.label(),
            entries,
            path,
        })
    })
    .await
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExtractMode {
    /// Intelligent, à côté de l'archive.
    Here,
    /// Dans un dossier au nom de l'archive (à côté, ou dans `dest`).
    Named,
    /// Intelligent, dans `dest`.
    Into,
    /// Exactement dans `dest`.
    Exact,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractRequest {
    job: u32,
    archive: String,
    mode: ExtractMode,
    dest: Option<String>,
    password: Option<String>,
    #[serde(default)]
    overwrite: Overwrite,
    selection: Option<Selection>,
}

#[tauri::command]
pub async fn extract(app: AppHandle, jobs: State<'_, Jobs>, req: ExtractRequest) -> CmdResult<ExtractReport> {
    let job = req.job;
    let cancel = jobs.start(job);
    let result = blocking(move || {
        let progress = JobProgress::new(app, job, cancel);
        let archive = PathBuf::from(&req.archive);
        let opts = ExtractOptions { password: req.password, overwrite: req.overwrite, selection: req.selection };
        let beside = archive.parent().unwrap_or(Path::new(".")).to_path_buf();
        let dest = req.dest.map(PathBuf::from);
        let missing = || winsar_core::Error::Unsupported("destination manquante");
        let report = match req.mode {
            ExtractMode::Here => winsar_core::extract_smart(&archive, &opts, &progress),
            ExtractMode::Into => winsar_core::extract_smart_in(&archive, &dest.ok_or_else(missing)?, &opts, &progress),
            ExtractMode::Named => {
                let target = winsar_core::named_destination(&archive, &dest.unwrap_or(beside));
                winsar_core::extract(&archive, &target, &opts, &progress)
            }
            ExtractMode::Exact => winsar_core::extract(&archive, &dest.ok_or_else(missing)?, &opts, &progress),
        };
        progress.flush();
        report
    })
    .await;
    jobs.finish(job);
    result
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateRequest {
    job: u32,
    sources: Vec<String>,
    dest: String,
    format: Format,
    level: u32,
    password: Option<String>,
    /// Découpe en volumes `.001`, `.002`… de cette taille.
    volume_size: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Created {
    /// L'archive, ou ses volumes dans l'ordre.
    paths: Vec<String>,
    /// Taille totale sur le disque.
    size: u64,
}

#[tauri::command]
pub async fn create(app: AppHandle, jobs: State<'_, Jobs>, req: CreateRequest) -> CmdResult<Created> {
    let job = req.job;
    let cancel = jobs.start(job);
    let result = blocking(move || {
        let progress = JobProgress::new(app, job, cancel);
        let sources: Vec<PathBuf> = req.sources.iter().map(PathBuf::from).collect();
        let dest = PathBuf::from(&req.dest);
        let opts = CreateOptions {
            format: req.format,
            level: req.level,
            password: req.password.filter(|p| !p.is_empty()),
            volume_size: req.volume_size.filter(|&s| s > 0),
        };
        let parts = winsar_core::create(&sources, &dest, &opts, &progress)?;
        progress.flush();
        created(&parts)
    })
    .await;
    jobs.finish(job);
    result
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConvertRequest {
    job: u32,
    archive: String,
    dest: String,
    /// Mot de passe de l'archive d'origine.
    password: Option<String>,
    format: Format,
    level: u32,
    /// Mot de passe de la nouvelle archive.
    new_password: Option<String>,
    volume_size: Option<u64>,
}

#[tauri::command]
pub async fn convert(app: AppHandle, jobs: State<'_, Jobs>, req: ConvertRequest) -> CmdResult<Created> {
    let job = req.job;
    let cancel = jobs.start(job);
    let result = blocking(move || {
        let progress = JobProgress::new(app, job, cancel);
        let opts = winsar_core::ConvertOptions {
            password: req.password,
            create: CreateOptions {
                format: req.format,
                level: req.level,
                password: req.new_password.filter(|p| !p.is_empty()),
                volume_size: req.volume_size.filter(|&s| s > 0),
            },
        };
        let parts = winsar_core::convert(Path::new(&req.archive), Path::new(&req.dest), &opts, &progress)?;
        progress.flush();
        created(&parts)
    })
    .await;
    jobs.finish(job);
    result
}

fn created(parts: &[PathBuf]) -> winsar_core::Result<Created> {
    let mut size = 0;
    for p in parts {
        size += std::fs::metadata(p)?.len();
    }
    let paths = parts.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    Ok(Created { paths, size })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TestRequest {
    job: u32,
    archive: String,
    password: Option<String>,
}

#[tauri::command]
pub async fn test_archive(
    app: AppHandle,
    jobs: State<'_, Jobs>,
    req: TestRequest,
) -> CmdResult<winsar_core::TestReport> {
    let job = req.job;
    let cancel = jobs.start(job);
    let result = blocking(move || {
        let progress = JobProgress::new(app, job, cancel);
        let report = winsar_core::test(Path::new(&req.archive), req.password.as_deref(), &progress);
        progress.flush();
        report
    })
    .await;
    jobs.finish(job);
    result
}

/// Notification Windows en fin de tâche lancée depuis l'Explorateur.
#[tauri::command]
pub async fn notify(title: String, body: String, folder: Option<String>) {
    #[cfg(windows)]
    {
        let _ = tauri::async_runtime::spawn_blocking(move || {
            crate::win::with_com(|| crate::win::toast(&title, &body, folder.as_deref().map(Path::new)))
        })
        .await;
    }
    #[cfg(not(windows))]
    let _ = (title, body, folder);
}

#[tauri::command]
pub fn cancel(jobs: State<'_, Jobs>, job: u32) {
    jobs.cancel(job);
}

/// Nom proposé pour une nouvelle archive, à côté des sources ou dans `dir`, jamais un fichier existant.
#[tauri::command]
pub fn suggest_archive_path(sources: Vec<String>, format: Format, dir: Option<String>) -> String {
    let sources: Vec<PathBuf> = sources.iter().map(PathBuf::from).collect();
    let path = winsar_core::default_archive_path(&sources, format);
    let Some(dir) = dir.map(PathBuf::from) else {
        return path.to_string_lossy().into_owned();
    };
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = format!(".{}", format.extension());
    // Le suffixe « (2) » posé ailleurs n'a pas de sens ici : on repart du nom de base.
    let stem = name.strip_suffix(&ext).unwrap_or(&name);
    let stem = match stem.rsplit_once(" (") {
        Some((base, n)) if n.ends_with(')') && n[..n.len() - 1].bytes().all(|b| b.is_ascii_digit()) => base,
        _ => stem,
    };
    (1u32..)
        .map(|n| if n == 1 { dir.join(format!("{stem}{ext}")) } else { dir.join(format!("{stem} ({n}){ext}")) })
        .find(|p| std::fs::symlink_metadata(p).is_err())
        .unwrap_or_else(|| dir.join(name))
        .to_string_lossy()
        .into_owned()
}

fn scratch_root() -> PathBuf {
    std::env::temp_dir().join("WinSAR")
}

/// Dossier temporaire neuf pour une ouverture ou un glisser-déposer.
fn scratch_dir() -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    scratch_root().join(format!("{}-{}", std::process::id(), COUNTER.fetch_add(1, Ordering::Relaxed)))
}

/// Au démarrage : les copies temporaires de plus de 12 h ne servent plus. On ne les
/// efface pas à la fermeture, l'Explorateur peut encore copier depuis elles.
pub fn clean_scratch() {
    let Ok(entries) = std::fs::read_dir(scratch_root()) else { return };
    let limit = std::time::Duration::from_secs(12 * 3600);
    for e in entries.flatten() {
        let old = e.metadata().and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().unwrap_or_default() > limit);
        if old {
            let _ = std::fs::remove_dir_all(e.path());
        }
    }
}

/// Extrait des éléments de l'archive dans un dossier temporaire neuf et rend leurs
/// chemins, dans l'ordre demandé. Chacun est extrait relativement à son propre
/// dossier : tous arrivent au premier niveau, même issus d'une recherche.
fn materialize(
    archive: &Path,
    items: &[String],
    password: Option<String>,
    progress: &dyn winsar_core::Progress,
) -> winsar_core::Result<Vec<PathBuf>> {
    let dir = scratch_dir();
    let mut groups: HashMap<String, Vec<String>> = HashMap::new();
    for item in items {
        let parent = item.trim_matches('/').rsplit_once('/').map_or("", |(p, _)| p).to_owned();
        groups.entry(parent).or_default().push(item.clone());
    }
    for (base, items) in groups {
        let opts = ExtractOptions {
            password: password.clone(),
            overwrite: Overwrite::Replace,
            selection: Some(Selection { base, items }),
        };
        winsar_core::extract(archive, &dir, &opts, progress)?;
    }
    Ok(items
        .iter()
        .filter_map(|i| winsar_core::safe_relative(i.trim_matches('/').rsplit('/').next().unwrap_or(i)))
        .map(|rel| dir.join(rel))
        .filter(|p| p.exists())
        .collect())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MaterializeRequest {
    job: u32,
    archive: String,
    items: Vec<String>,
    password: Option<String>,
}

/// Copie temporaire d'éléments de l'archive, avec progression : archive imbriquée
/// à ouvrir, fichiers à prévisualiser ou dont calculer les sommes de contrôle.
#[tauri::command]
pub async fn materialize_entries(
    app: AppHandle,
    jobs: State<'_, Jobs>,
    req: MaterializeRequest,
) -> CmdResult<Vec<String>> {
    let job = req.job;
    let cancel = jobs.start(job);
    let result = blocking(move || {
        let progress = JobProgress::new(app, job, cancel);
        let files = materialize(Path::new(&req.archive), &req.items, req.password, &progress)?;
        progress.flush();
        Ok(files.into_iter().map(|p| p.to_string_lossy().into_owned()).collect())
    })
    .await;
    jobs.finish(job);
    result
}

/// Ouvre un fichier de l'archive avec son application par défaut,
/// via une copie temporaire (comme un double-clic dans WinRAR).
#[tauri::command]
pub async fn open_entry(archive: String, entry: String, password: Option<String>) -> CmdResult<()> {
    let file = blocking(move || {
        let files = materialize(Path::new(&archive), &[entry], password, &winsar_core::NoProgress)?;
        files.into_iter().next().ok_or(winsar_core::Error::Unsupported("nom de fichier invalide"))
    })
    .await?;
    tauri_plugin_opener::open_path(file, None::<&str>).map_err(|e| CmdError::other(e.to_string()))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateRequest {
    job: u32,
    archive: String,
    /// Dossier de l'archive où déposer les ajouts ("" : racine).
    base: String,
    #[serde(default)]
    add: Vec<String>,
    #[serde(default)]
    remove: Vec<String>,
    password: Option<String>,
    level: Option<u32>,
}

#[tauri::command]
pub async fn update(app: AppHandle, jobs: State<'_, Jobs>, req: UpdateRequest) -> CmdResult<()> {
    let job = req.job;
    let cancel = jobs.start(job);
    let result = blocking(move || {
        let progress = JobProgress::new(app, job, cancel);
        let add: Vec<PathBuf> = req.add.iter().map(PathBuf::from).collect();
        let opts = winsar_core::UpdateOptions {
            password: req.password.filter(|p| !p.is_empty()),
            level: req.level.unwrap_or(6),
        };
        winsar_core::update(Path::new(&req.archive), &req.base, &add, &req.remove, &opts, &progress)?;
        progress.flush();
        Ok(())
    })
    .await;
    jobs.finish(job);
    result
}

/// Icônes système par type, en `data:` URL. Clés : extension (`.pdf`), `""`, ou `/` (dossier).
#[tauri::command]
pub async fn file_icons(keys: Vec<String>) -> HashMap<String, String> {
    #[cfg(windows)]
    {
        use base64::Engine;
        tauri::async_runtime::spawn_blocking(move || {
            crate::win::with_com(|| {
                keys.into_iter()
                    .filter_map(|key| {
                        let png = crate::win::type_icon_png(&key)?;
                        let b64 = base64::engine::general_purpose::STANDARD.encode(png);
                        Some((key, format!("data:image/png;base64,{b64}")))
                    })
                    .collect()
            })
        })
        .await
        .unwrap_or_default()
    }
    #[cfg(not(windows))]
    {
        let _ = keys;
        HashMap::new()
    }
}

/// Glisser des éléments de l'archive vers l'Explorateur : extraction dans un dossier
/// temporaire, puis glisser-déposer natif de ces fichiers. Rend `true` s'ils ont été déposés.
#[tauri::command]
pub async fn drag_out(
    window: tauri::WebviewWindow,
    archive: String,
    items: Vec<String>,
    password: Option<String>,
) -> CmdResult<bool> {
    let files = blocking(move || materialize(Path::new(&archive), &items, password, &winsar_core::NoProgress)).await?;
    drag_native(window, files).await
}

/// Glisser-déposer natif de fichiers déjà présents sur le disque.
pub async fn drag_native(window: tauri::WebviewWindow, files: Vec<PathBuf>) -> CmdResult<bool> {
    if files.is_empty() {
        return Ok(false);
    }
    #[cfg(windows)]
    {
        let hwnd = window.hwnd().map_err(|e| CmdError::other(e.to_string()))?.0 as isize;
        let (tx, rx) = std::sync::mpsc::channel();
        // DoDragDrop doit tourner sur le thread de la fenêtre (STA, OLE initialisé).
        window
            .run_on_main_thread(move || {
                let _ = tx.send(crate::win::drag_files(hwnd, &files).map_err(|e| e.to_string()));
            })
            .map_err(|e| CmdError::other(e.to_string()))?;
        tauri::async_runtime::spawn_blocking(move || rx.recv())
            .await
            .map_err(|e| CmdError::other(e.to_string()))?
            .map_err(|e| CmdError::other(e.to_string()))?
            .map_err(CmdError::other)
    }
    #[cfg(not(windows))]
    {
        let _ = (window, files);
        Ok(false)
    }
}

#[tauri::command]
pub fn reveal(path: String) -> CmdResult<()> {
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(|e| CmdError::other(e.to_string()))
}

/// Propriétés Windows d'un fichier ou d'un dossier (feuille native, comme Alt+Entrée).
#[tauri::command]
pub fn properties(window: tauri::WebviewWindow, path: String) -> CmdResult<()> {
    #[cfg(windows)]
    {
        let hwnd = window.hwnd().map_err(|e| CmdError::other(e.to_string()))?.0 as isize;
        window
            .run_on_main_thread(move || {
                let _ = crate::win::properties(hwnd, &path);
            })
            .map_err(|e| CmdError::other(e.to_string()))
    }
    #[cfg(not(windows))]
    {
        let _ = (window, path);
        Ok(())
    }
}

#[tauri::command]
pub fn open_path(path: String) -> CmdResult<()> {
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(|e| CmdError::other(e.to_string()))
}

#[derive(Serialize)]
pub struct Accent {
    light: String,
    dark: String,
    /// Couleur choisie dans Paramètres › Personnalisation, avant déclinaison.
    base: String,
}

/// Couleur d'accentuation de Windows, déclinée comme le fait le système :
/// teinte foncée sur fond clair, teinte claire sur fond sombre.
#[tauri::command]
pub fn system_accent() -> Option<Accent> {
    #[cfg(windows)]
    {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        let key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent")
            .ok()?;
        let palette = key.get_raw_value("AccentPalette").ok()?.bytes;
        // 8 couleurs RGBA : Light3, Light2, Light1, Base, Dark1, Dark2, Dark3, (inutilisée).
        let color = |i: usize| {
            let c = palette.get(i * 4..i * 4 + 3)?;
            Some(format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2]))
        };
        Some(Accent { light: color(4)?, dark: color(1)?, base: color(3)? })
    }
    #[cfg(not(windows))]
    {
        None
    }
}
