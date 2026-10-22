//! Réglages : un fichier JSON dans le dossier de données de l'application, dont le
//! schéma appartient à l'interface. Rust n'applique que ce qui touche Windows :
//! entrées du menu contextuel et associations de fichiers.

use std::collections::HashMap;

use serde::Serialize;
use tauri::{AppHandle, Manager};

use crate::commands::{CmdError, CmdResult};

fn settings_file(app: &AppHandle) -> CmdResult<std::path::PathBuf> {
    let dir = app.path().app_data_dir().map_err(|e| CmdError::other(e.to_string()))?;
    Ok(dir.join("settings.json"))
}

/// Réglages enregistrés, `null` au premier lancement (l'interface applique ses défauts).
#[tauri::command]
pub fn settings_load(app: AppHandle) -> serde_json::Value {
    settings_file(&app)
        .ok()
        .and_then(|f| std::fs::read(f).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(serde_json::Value::Null)
}

#[tauri::command]
pub fn settings_save(app: AppHandle, value: serde_json::Value) -> CmdResult<()> {
    let file = settings_file(&app)?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| CmdError::other(e.to_string()))?;
    }
    let json = serde_json::to_vec_pretty(&value).map_err(|e| CmdError::other(e.to_string()))?;
    // Écriture puis renommage : un fichier de réglages n'est jamais à moitié écrit.
    let tmp = file.with_extension("json.tmp");
    std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, &file)).map_err(|e| CmdError::other(e.to_string()))
}

/// Entrées masquées du menu contextuel, lues par winsar_shell.dll à chaque affichage.
/// Bit n : n-ième entrée de `winsar_shell::verbs::Verb::ALL`.
#[tauri::command]
pub fn set_context_menu_hidden(mask: u32) -> CmdResult<()> {
    #[cfg(windows)]
    {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(r"Software\WinSAR")
            .map_err(|e| CmdError::other(e.to_string()))?;
        key.set_value("ContextMenuHidden", &mask).map_err(|e| CmdError::other(e.to_string()))?;
    }
    #[cfg(not(windows))]
    let _ = mask;
    Ok(())
}

#[cfg(windows)]
const PROGID: &str = "WinSAR.Archive";
#[cfg(windows)]
const CAPABILITIES: &str = r"Software\WinSAR\Capabilities";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTypeStatus {
    /// WinSAR est proposé pour ce type (« Ouvrir avec », paramètres de Windows).
    registered: bool,
    /// WinSAR est l'application par défaut, choisie par l'utilisateur.
    default: bool,
    /// Application par défaut actuelle (ProgID), pour information.
    current: Option<String>,
}

#[tauri::command]
pub fn file_types_status(exts: Vec<String>) -> HashMap<String, FileTypeStatus> {
    #[cfg(windows)]
    {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        let assoc = RegKey::predef(HKEY_CURRENT_USER).open_subkey(format!(r"{CAPABILITIES}\FileAssociations")).ok();
        exts.into_iter()
            .map(|ext| {
                let registered = assoc.as_ref().is_some_and(|k| k.get_value::<String, _>(&ext).is_ok());
                let current = crate::win::default_progid(&ext);
                let default = current.as_deref() == Some(PROGID);
                (ext, FileTypeStatus { registered, default, current })
            })
            .collect()
    }
    #[cfg(not(windows))]
    {
        let _ = exts;
        HashMap::new()
    }
}

/// Déclare (ou retire) WinSAR comme application capable d'ouvrir ces extensions.
/// Windows interdit à une application de se rendre elle-même « par défaut » :
/// l'utilisateur confirme ensuite dans Paramètres (voir `open_default_apps`).
#[tauri::command]
pub fn register_file_types(exts: Vec<String>, enable: bool) -> CmdResult<()> {
    #[cfg(windows)]
    {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        let err = |e: std::io::Error| CmdError::other(e.to_string());
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let exe = std::env::current_exe().map_err(err)?;
        let exe = exe.display();

        let (prog, _) = hkcu.create_subkey(format!(r"Software\Classes\{PROGID}")).map_err(err)?;
        prog.set_value("", &"Archive").map_err(err)?;
        // Icône de document des archives (archive.ico, posée à côté de l'exécutable) ;
        // à défaut, celle de l'application.
        let doc_icon =
            std::env::current_exe().ok().and_then(|p| Some(p.parent()?.join("archive.ico"))).filter(|p| p.is_file());
        let icon_value = doc_icon.map_or_else(|| format!("{exe},0"), |p| p.display().to_string());
        let (icon, _) = prog.create_subkey("DefaultIcon").map_err(err)?;
        icon.set_value("", &icon_value).map_err(err)?;
        let (cmd, _) = prog.create_subkey(r"shell\open\command").map_err(err)?;
        cmd.set_value("", &format!("\"{exe}\" \"%1\"")).map_err(err)?;

        let (caps, _) = hkcu.create_subkey(CAPABILITIES).map_err(err)?;
        caps.set_value("ApplicationName", &"WinSAR").map_err(err)?;
        caps.set_value("ApplicationDescription", &"Archives rapides, intégrées à l'Explorateur").map_err(err)?;
        caps.set_value("ApplicationIcon", &format!("{exe},0")).map_err(err)?;
        let (assoc, _) = caps.create_subkey("FileAssociations").map_err(err)?;
        let (apps, _) = hkcu.create_subkey(r"Software\RegisteredApplications").map_err(err)?;
        apps.set_value("WinSAR", &CAPABILITIES).map_err(err)?;

        for ext in &exts {
            let (open_with, _) = hkcu.create_subkey(format!(r"Software\Classes\{ext}\OpenWithProgids")).map_err(err)?;
            if enable {
                assoc.set_value(ext, &PROGID).map_err(err)?;
                open_with.set_value(PROGID, &"").map_err(err)?;
            } else {
                let _ = assoc.delete_value(ext);
                let _ = open_with.delete_value(PROGID);
            }
        }
        crate::win::notify_assoc_changed();
    }
    #[cfg(not(windows))]
    let _ = (exts, enable);
    Ok(())
}

/// Extensions qui reçoivent le sous-menu d'extraction du menu classique.
#[cfg(windows)]
const ARCHIVE_EXTS: &[&str] =
    &[".zip", ".jar", ".7z", ".rar", ".tar", ".gz", ".tgz", ".bz2", ".tbz2", ".xz", ".txz", ".zst", ".tzst", ".001"];
#[cfg(windows)]
const CLASSIC_ARCHIVE_VERB: &str = "WinSAR.Archive";
#[cfg(windows)]
const CLASSIC_COMPRESS_VERB: &str = "WinSAR.Compress";

/// Menu contextuel classique (« Afficher plus d'options ») : de simples verbes de
/// registre, sans DLL ni package signé. C'est le repli de l'installeur quand le
/// package MSIX n'a pas pu être enregistré (certificat non approuvé).
#[cfg(windows)]
pub fn set_classic_menu(enable: bool) -> std::io::Result<()> {
    use winreg::{RegKey, enums::HKEY_CURRENT_USER};
    let classes = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(r"Software\Classes", winreg::enums::KEY_READ | winreg::enums::KEY_WRITE)?;
    let archive_keys: Vec<String> =
        ARCHIVE_EXTS.iter().map(|e| format!(r"SystemFileAssociations\{e}\shell\{CLASSIC_ARCHIVE_VERB}")).collect();
    let compress_keys = [r"*\shell", r"Directory\shell"].map(|k| format!(r"{k}\{CLASSIC_COMPRESS_VERB}"));
    for key in archive_keys.iter().chain(&compress_keys) {
        let _ = classes.delete_subkey_all(key);
    }
    if enable {
        let exe = std::env::current_exe()?;
        let exe = exe.display();
        // Sous-menu en cascade : MUIVerb + SubCommands vide, entrées triées par nom de clé.
        let menu = |key: &str, title: &str, entries: &[(&str, &str, &str)]| -> std::io::Result<()> {
            let (root, _) = classes.create_subkey(key)?;
            root.set_value("MUIVerb", &title)?;
            root.set_value("Icon", &format!("{exe},0"))?;
            root.set_value("SubCommands", &"")?;
            for (id, label, args) in entries {
                let (item, _) = root.create_subkey(format!(r"shell\{id}"))?;
                item.set_value("MUIVerb", label)?;
                let (cmd, _) = item.create_subkey("command")?;
                cmd.set_value("", &format!("\"{exe}\" {args} \"%1\""))?;
            }
            Ok(())
        };
        for key in &archive_keys {
            menu(
                key,
                "WinSAR",
                &[
                    ("1here", "Extraire ici", "--extract-here"),
                    ("2named", "Extraire dans un dossier à son nom", "--extract-named"),
                    ("3to", "Extraire vers…", "--extract-to"),
                    ("4open", "Ouvrir avec WinSAR", ""),
                ],
            )?;
        }
        for key in &compress_keys {
            menu(
                key,
                "Compresser avec WinSAR",
                &[
                    ("1zip", "Ajouter à une archive ZIP", "--compress zip"),
                    ("27z", "Ajouter à une archive 7z", "--compress 7z"),
                    ("3dialog", "Ajouter à une archive…", "--compress-dialog"),
                ],
            )?;
        }
    }
    crate::win::notify_assoc_changed();
    Ok(())
}

#[tauri::command]
pub fn classic_menu_status() -> bool {
    #[cfg(windows)]
    {
        use winreg::{RegKey, enums::HKEY_CURRENT_USER};
        RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey(format!(r"Software\Classes\Directory\shell\{CLASSIC_COMPRESS_VERB}"))
            .is_ok()
    }
    #[cfg(not(windows))]
    false
}

#[tauri::command]
pub fn set_classic_menu_enabled(enable: bool) -> CmdResult<()> {
    #[cfg(windows)]
    set_classic_menu(enable).map_err(|e| CmdError::other(e.to_string()))?;
    #[cfg(not(windows))]
    let _ = enable;
    Ok(())
}

/// Page « Applications par défaut » de Windows, directement sur WinSAR.
#[tauri::command]
pub fn open_default_apps() -> CmdResult<()> {
    tauri_plugin_opener::open_url("ms-settings:defaultapps?registeredAppUser=WinSAR", None::<&str>)
        .map_err(|e| CmdError::other(e.to_string()))
}

#[tauri::command]
pub fn open_url(url: String) -> CmdResult<()> {
    if !url.starts_with("https://") {
        return Err(CmdError::other("seuls les liens https sont ouverts"));
    }
    tauri_plugin_opener::open_url(url, None::<&str>).map_err(|e| CmdError::other(e.to_string()))
}
