//! Archives récentes : liste propre à WinSAR (écran d'accueil) et liste de raccourcis
//! de Windows (clic droit sur l'icône de la barre des tâches, catégorie « Récent »).

use std::{fs, path::PathBuf, sync::Mutex};

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

const MAX: usize = 12;

#[derive(Default)]
pub struct Recent(Mutex<()>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentItem {
    path: String,
    name: String,
    size: u64,
    /// Dernière ouverture, en secondes Unix.
    opened: i64,
}

#[derive(serde::Deserialize, Serialize, Clone)]
struct Stored {
    path: String,
    opened: i64,
}

fn file(app: &AppHandle) -> Option<PathBuf> {
    Some(app.path().app_data_dir().ok()?.join("recent.json"))
}

fn load(app: &AppHandle) -> Vec<Stored> {
    file(app).and_then(|f| fs::read(f).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(app: &AppHandle, items: &[Stored]) {
    if let Some(f) = file(app) {
        if let Some(dir) = f.parent() {
            let _ = fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_vec_pretty(items) {
            let _ = fs::write(f, json);
        }
    }
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// Archives encore présentes sur le disque, de la plus récente à la plus ancienne.
#[tauri::command]
pub fn recent_list(app: AppHandle, lock: State<'_, Recent>) -> Vec<RecentItem> {
    let _guard = lock.0.lock().unwrap();
    load(&app)
        .into_iter()
        .filter_map(|s| {
            let meta = fs::metadata(&s.path).ok()?;
            let name = PathBuf::from(&s.path).file_name()?.to_string_lossy().into_owned();
            Some(RecentItem { name, size: meta.len(), opened: s.opened, path: s.path })
        })
        .collect()
}

#[tauri::command]
pub fn recent_add(app: AppHandle, lock: State<'_, Recent>, path: String) {
    let _guard = lock.0.lock().unwrap();
    let mut items = load(&app);
    items.retain(|s| !s.path.eq_ignore_ascii_case(&path));
    items.insert(0, Stored { path: path.clone(), opened: now() });
    items.truncate(MAX);
    save(&app, &items);
    #[cfg(windows)]
    crate::win::add_to_recent_docs(&path);
}

#[tauri::command]
pub fn recent_remove(app: AppHandle, lock: State<'_, Recent>, path: String) {
    let _guard = lock.0.lock().unwrap();
    let mut items = load(&app);
    items.retain(|s| !s.path.eq_ignore_ascii_case(&path));
    save(&app, &items);
}
