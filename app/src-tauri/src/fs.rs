//! Navigation dans le système de fichiers : Lynk est un explorateur à part entière,
//! les archives n'en sont qu'un type de dossier de plus.

use std::{collections::HashMap, path::PathBuf, time::UNIX_EPOCH};

use serde::Serialize;

use crate::commands::{CmdError, CmdResult};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsEntry {
    name: String,
    path: String,
    is_dir: bool,
    size: u64,
    /// Secondes Unix.
    modified: Option<i64>,
    /// Attribut « caché » ou « système » de Windows.
    hidden: bool,
}

/// Contenu d'un dossier. Les éléments illisibles (droits) sont ignorés en silence,
/// comme dans l'Explorateur ; seul un dossier inaccessible est une erreur.
#[tauri::command]
pub async fn list_dir(path: String, show_hidden: bool) -> CmdResult<Vec<FsEntry>> {
    tauri::async_runtime::spawn_blocking(move || {
        let read = std::fs::read_dir(&path).map_err(|e| CmdError::other(format!("{path} : {e}")))?;
        let mut out = Vec::new();
        for entry in read.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            #[cfg(windows)]
            let hidden = {
                use std::os::windows::fs::MetadataExt;
                meta.file_attributes() & 0x6 != 0 // FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM
            };
            #[cfg(not(windows))]
            let hidden = entry.file_name().to_string_lossy().starts_with('.');
            if hidden && !show_hidden {
                continue;
            }
            // Jonction ou lien vers un dossier : on suit, comme l'Explorateur.
            let is_dir = meta.is_dir() || (meta.file_type().is_symlink() && entry.path().is_dir());
            out.push(FsEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: entry.path().to_string_lossy().into_owned(),
                is_dir,
                size: if is_dir { 0 } else { meta.len() },
                modified: meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64),
                hidden,
            });
        }
        Ok(out)
    })
    .await
    .map_err(|e| CmdError::other(e.to_string()))?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    /// Nom affiché par l'Explorateur (« Téléchargements », « Disque local (C:) »).
    name: String,
    path: String,
    /// Icône système en `data:` URL.
    icon: Option<String>,
    /// Lecteurs seulement.
    total: Option<u64>,
    free: Option<u64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Places {
    folders: Vec<Place>,
    drives: Vec<Place>,
    home: String,
}

/// Accès rapide : dossiers connus de l'utilisateur et lecteurs, avec les noms et icônes de Windows.
#[tauri::command]
pub async fn places() -> Places {
    tauri::async_runtime::spawn_blocking(|| {
        #[cfg(windows)]
        {
            crate::win::with_com(|| {
                let place = |path: PathBuf, total: Option<u64>, free: Option<u64>| {
                    let p = path.to_string_lossy().into_owned();
                    Place {
                        name: crate::win::display_name(&p).unwrap_or_else(|| p.clone()),
                        icon: crate::win::path_icon(&p),
                        path: p,
                        total,
                        free,
                    }
                };
                let folders = crate::win::known_folders().into_iter().map(|p| place(p, None, None)).collect();
                let drives =
                    crate::win::drives().into_iter().map(|(root, total, free)| place(root, total, free)).collect();
                let home = std::env::var("USERPROFILE").unwrap_or_default();
                Places { folders, drives, home }
            })
        }
        #[cfg(not(windows))]
        {
            Places { folders: Vec::new(), drives: Vec::new(), home: std::env::var("HOME").unwrap_or_default() }
        }
    })
    .await
    .unwrap_or(Places { folders: Vec::new(), drives: Vec::new(), home: String::new() })
}

/// Icônes propres à certains fichiers (programmes, raccourcis, icônes) : l'icône par
/// type ne suffit pas pour eux. Clé : chemin complet.
#[tauri::command]
pub async fn path_icons(paths: Vec<String>) -> HashMap<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(windows)]
        {
            crate::win::with_com(|| {
                paths
                    .into_iter()
                    .filter_map(|p| {
                        let icon = crate::win::path_icon(&p)?;
                        Some((p, icon))
                    })
                    .collect()
            })
        }
        #[cfg(not(windows))]
        {
            let _ = paths;
            HashMap::new()
        }
    })
    .await
    .unwrap_or_default()
}

/// Glisser-déposer natif de vrais fichiers (mode explorateur).
#[tauri::command]
pub async fn drag_paths(window: tauri::WebviewWindow, paths: Vec<String>) -> CmdResult<bool> {
    let files: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).filter(|p| p.exists()).collect();
    crate::commands::drag_native(window, files).await
}

/// Le dossier parent existe-t-il encore, et le chemin est-il un dossier ?
#[tauri::command]
pub fn path_kind(path: String) -> &'static str {
    let p = PathBuf::from(path);
    if p.is_dir() {
        "dir"
    } else if p.is_file() {
        "file"
    } else {
        "missing"
    }
}
