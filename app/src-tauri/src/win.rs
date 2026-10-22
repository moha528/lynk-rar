//! Intégrations Windows de l'application : icônes du shell, glisser-déposer sortant,
//! propriétés des fichiers.

use std::{ffi::c_void, mem::size_of, path::PathBuf};

use windows::Win32::{
    Foundation::HWND,
    Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject,
        GetDIBits, GetObjectW, HBITMAP, HDC, HGDIOBJ,
    },
    Storage::FileSystem::{FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_FLAGS_AND_ATTRIBUTES},
    System::{
        Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize, IDataObject},
        Ole::{DROPEFFECT_COPY, IDropSource, OleInitialize},
    },
    UI::{
        Shell::{
            BHID_DataObject, ILCreateFromPathW, ILFree, SHARD_PATHW, SHAddToRecentDocs,
            SHCreateShellItemArrayFromIDLists, SHDoDragDrop, SHFILEINFOW, SHGFI_DISPLAYNAME, SHGFI_ICON,
            SHGFI_LARGEICON, SHGFI_USEFILEATTRIBUTES, SHGetFileInfoW, SHOP_FILEPATH, SHObjectProperties,
        },
        WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO},
    },
};
use windows_core::{HSTRING, PCWSTR};

/// Icône système d'un type de fichier, en PNG RGBA (32 px, affichée à 20 px : nette en HiDPI).
/// `key` est une extension (`.pdf`), `""` sans extension, ou `/` pour un dossier.
pub fn type_icon_png(key: &str) -> Option<Vec<u8>> {
    let (name, attrs) = match key {
        "/" => ("dossier".to_owned(), FILE_ATTRIBUTE_DIRECTORY),
        ext => (format!("fichier{ext}"), FILE_ATTRIBUTE_NORMAL),
    };
    let mut info = SHFILEINFOW::default();
    // USEFILEATTRIBUTES : rien n'est lu sur le disque, seul le type compte.
    let ok = unsafe {
        SHGetFileInfoW(
            &HSTRING::from(name),
            attrs,
            Some(&mut info),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON | SHGFI_USEFILEATTRIBUTES,
        )
    };
    if ok == 0 || info.hIcon.is_invalid() {
        return None;
    }
    let png = icon_to_png(info.hIcon);
    unsafe {
        let _ = DestroyIcon(info.hIcon);
    }
    png
}

fn icon_to_png(icon: HICON) -> Option<Vec<u8>> {
    let mut ii = ICONINFO::default();
    unsafe { GetIconInfo(icon, &mut ii) }.ok()?;
    let result = (|| {
        if ii.hbmColor.is_invalid() {
            return None; // Icône monochrome : rarissime aujourd'hui.
        }
        let mut bm = BITMAP::default();
        let got = unsafe {
            GetObjectW(HGDIOBJ(ii.hbmColor.0), size_of::<BITMAP>() as i32, Some(&mut bm as *mut _ as *mut c_void))
        };
        if got == 0 {
            return None;
        }
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        let dc = unsafe { CreateCompatibleDC(None) };
        let mut rgba = dib_pixels(dc, ii.hbmColor, w, h)?;
        if rgba.chunks(4).all(|p| p[3] == 0) {
            // Ancienne icône sans canal alpha : la transparence vient du masque.
            let mask = dib_pixels(dc, ii.hbmMask, w, h)?;
            for (p, m) in rgba.chunks_mut(4).zip(mask.chunks(4)) {
                p[3] = if m[0] == 0 { 255 } else { 0 };
            }
        }
        unsafe {
            let _ = DeleteDC(dc);
        }
        for p in rgba.chunks_mut(4) {
            p.swap(0, 2); // BGRA → RGBA
        }
        encode_png(&rgba, w as u32, h as u32)
    })();
    unsafe {
        let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0));
        let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0));
    }
    result
}

fn dib_pixels(dc: HDC, bitmap: HBITMAP, w: i32, h: i32) -> Option<Vec<u8>> {
    let mut bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h, // négatif : lignes de haut en bas
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut pixels = vec![0u8; (w * h * 4) as usize];
    let lines = unsafe {
        GetDIBits(dc, bitmap, 0, h as u32, Some(pixels.as_mut_ptr() as *mut c_void), &mut bi, DIB_RGB_COLORS)
    };
    (lines == h).then_some(pixels)
}

fn encode_png(rgba: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, w, h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(rgba).ok()?;
    writer.finish().ok()?;
    Some(out)
}

/// COM le temps d'un appel sur un thread de travail (SHGetFileInfo l'exige).
pub fn with_com<T>(f: impl FnOnce() -> T) -> T {
    let init = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let result = f();
    if init.is_ok() {
        unsafe { CoUninitialize() };
    }
    result
}

/// ProgID de l'application par défaut pour une extension (choix de l'utilisateur compris).
pub fn default_progid(ext: &str) -> Option<String> {
    use windows::Win32::UI::Shell::{ASSOCF_NONE, ASSOCSTR_PROGID, AssocQueryStringW};
    let ext = HSTRING::from(ext);
    let mut len = 0u32;
    unsafe {
        let _ = AssocQueryStringW(ASSOCF_NONE, ASSOCSTR_PROGID, &ext, windows_core::PCWSTR::null(), None, &mut len);
        if len == 0 {
            return None;
        }
        let mut buf = vec![0u16; len as usize];
        AssocQueryStringW(
            ASSOCF_NONE,
            ASSOCSTR_PROGID,
            &ext,
            windows_core::PCWSTR::null(),
            Some(windows_core::PWSTR(buf.as_mut_ptr())),
            &mut len,
        )
        .ok()
        .ok()?;
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf16_lossy(&buf[..end]))
    }
}

/// Prévient l'Explorateur que les associations ont changé (icônes, « Ouvrir avec »).
pub fn notify_assoc_changed() {
    use windows::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, SHCNF_IDLIST, None, None) };
}

fn png_data_url(png: Vec<u8>) -> String {
    use base64::Engine;
    format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png))
}

/// Icône d'un chemin réel (dossier spécial, lecteur, programme, raccourci…).
pub fn path_icon(path: &str) -> Option<String> {
    let mut info = SHFILEINFOW::default();
    let ok = unsafe {
        SHGetFileInfoW(
            &HSTRING::from(path),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        )
    };
    if ok == 0 || info.hIcon.is_invalid() {
        return None;
    }
    let png = icon_to_png(info.hIcon);
    unsafe {
        let _ = DestroyIcon(info.hIcon);
    }
    png.map(png_data_url)
}

/// Nom affiché par l'Explorateur : « Téléchargements », « Disque local (C:) ».
pub fn display_name(path: &str) -> Option<String> {
    let mut info = SHFILEINFOW::default();
    let ok = unsafe {
        SHGetFileInfoW(
            &HSTRING::from(path),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_DISPLAYNAME,
        )
    };
    if ok == 0 {
        return None;
    }
    let len = info.szDisplayName.iter().position(|&c| c == 0).unwrap_or(info.szDisplayName.len());
    let name = String::from_utf16_lossy(&info.szDisplayName[..len]);
    (!name.is_empty()).then_some(name)
}

/// Dossiers de l'utilisateur, dans l'ordre de l'accès rapide de l'Explorateur.
pub fn known_folders() -> Vec<PathBuf> {
    use windows::Win32::UI::Shell::{
        FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Videos,
        KF_FLAG_DEFAULT, SHGetKnownFolderPath,
    };
    [FOLDERID_Desktop, FOLDERID_Downloads, FOLDERID_Documents, FOLDERID_Pictures, FOLDERID_Music, FOLDERID_Videos]
        .iter()
        .filter_map(|id| unsafe {
            let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
            let s = p.to_string().ok();
            windows::Win32::System::Com::CoTaskMemFree(Some(p.0 as *const c_void));
            s.map(PathBuf::from)
        })
        .filter(|p| p.is_dir())
        .collect()
}

/// Lecteurs montés : racine, taille totale, espace libre.
pub fn drives() -> Vec<(PathBuf, Option<u64>, Option<u64>)> {
    use windows::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetLogicalDrives};
    let mask = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| {
            let root = format!("{}:\\", (b'A' + i) as char);
            let (mut free, mut total) = (0u64, 0u64);
            // Lecteur vide (DVD, carte) : pas de taille, mais on le montre quand même.
            let sized =
                unsafe { GetDiskFreeSpaceExW(&HSTRING::from(root.as_str()), Some(&mut free), Some(&mut total), None) }
                    .is_ok();
            (PathBuf::from(root), sized.then_some(total), sized.then_some(free))
        })
        .collect()
}

/// Inscrit l'archive dans les documents récents : elle apparaît dans la liste de
/// raccourcis de WinSAR (clic droit sur la barre des tâches) dès que l'installeur a
/// associé le type de fichier à l'application.
pub fn add_to_recent_docs(path: &str) {
    let wide = HSTRING::from(path);
    unsafe { SHAddToRecentDocs(SHARD_PATHW.0 as u32, Some(wide.as_ptr() as *const c_void)) };
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// `C:\Mes fichiers\x` → `file:///C:/Mes%20fichiers/x`.
fn file_url(path: &std::path::Path) -> String {
    let mut url = String::from("file:///");
    for b in path.to_string_lossy().replace('\\', "/").bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b':' | b'-' | b'_' | b'.' | b'~' => url.push(b as char),
            b => url.push_str(&format!("%{b:02X}")),
        }
    }
    url
}

/// Identifiant d'application des notifications. L'installeur pose `dev.winsar.app` sur le
/// raccourci du menu Démarrer ; hors installation, Windows n'afficherait rien avec cet
/// identifiant, on emprunte alors celui de PowerShell (pratique courante en développement).
fn toast_app_id() -> &'static str {
    if cfg!(debug_assertions) {
        r"{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}\WindowsPowerShell\v1.0\powershell.exe"
    } else {
        "dev.winsar.app"
    }
}

/// Notification Windows native ; cliquer dessus (ou sur le bouton) ouvre `folder`.
pub fn toast(title: &str, body: &str, folder: Option<&std::path::Path>) -> windows_core::Result<()> {
    use windows::{
        Data::Xml::Dom::XmlDocument,
        UI::Notifications::{ToastNotification, ToastNotificationManager},
    };
    let url = folder.map(|f| xml_escape(&file_url(f)));
    let launch = url.as_deref().map(|u| format!(r#" activationType="protocol" launch="{u}""#)).unwrap_or_default();
    let actions = url
        .as_deref()
        .map(|u| {
            format!(
                r#"<actions><action content="Ouvrir le dossier" activationType="protocol" arguments="{u}"/></actions>"#
            )
        })
        .unwrap_or_default();
    let xml = format!(
        r#"<toast{launch}><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual>{actions}</toast>"#,
        xml_escape(title),
        xml_escape(body),
    );
    let doc = XmlDocument::new()?;
    doc.LoadXml(&HSTRING::from(xml))?;
    let notification = ToastNotification::CreateToastNotification(&doc)?;
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(toast_app_id()))?.Show(&notification)
}

/// Glisser-déposer natif de fichiers vers l'Explorateur (ou toute autre application).
/// Bloquant tant que l'utilisateur n'a pas lâché : à appeler sur le thread de l'interface.
pub fn drag_files(hwnd: isize, files: &[PathBuf]) -> windows_core::Result<bool> {
    unsafe {
        // Déjà fait par la boucle d'événements en principe ; sans effet si c'est le cas.
        let _ = OleInitialize(None);
        let pidls: Vec<_> =
            files.iter().map(|f| ILCreateFromPathW(&HSTRING::from(f.as_os_str()))).filter(|p| !p.is_null()).collect();
        let result = (|| {
            let consts: Vec<_> = pidls.iter().map(|p| *p as *const _).collect();
            let items = SHCreateShellItemArrayFromIDLists(&consts)?;
            // Objet de données du shell : CF_HDROP, noms, et image de glissement fournis.
            let data: IDataObject = items.BindToHandler(None, &BHID_DataObject)?;
            let effect = SHDoDragDrop(Some(HWND(hwnd as *mut c_void)), &data, None::<&IDropSource>, DROPEFFECT_COPY)?;
            Ok(effect.0 != 0)
        })();
        for p in pidls {
            ILFree(Some(p));
        }
        result
    }
}

/// Feuille de propriétés de Windows d'un fichier ou d'un dossier, comme Alt+Entrée dans
/// l'Explorateur. Le shell l'affiche sans bloquer : à appeler sur le thread de l'interface.
pub fn properties(hwnd: isize, path: &str) -> windows_core::Result<()> {
    unsafe {
        SHObjectProperties(Some(HWND(hwnd as *mut c_void)), SHOP_FILEPATH, &HSTRING::from(path), PCWSTR::null()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les appels au shell en parallèle (icônes, noms) échouent par intermittence entre tests.
    static SHELL: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn places_have_native_names_and_icons() {
        let _shell = SHELL.lock().unwrap_or_else(|e| e.into_inner());
        with_com(|| {
            let folders = known_folders();
            assert!(folders.len() >= 3, "{folders:?}");
            let drives = drives();
            let c = drives.iter().find(|(p, _, _)| p.to_string_lossy() == "C:\\").expect("lecteur C:");
            assert!(c.1.is_some_and(|total| total > 0));
            let name = display_name("C:\\").unwrap();
            assert!(name.contains("C:"), "{name}");
            assert!(path_icon(&folders[0].to_string_lossy()).is_some_and(|u| u.starts_with("data:image/png;base64,")));
        });
    }

    #[test]
    fn file_urls_are_percent_encoded() {
        assert_eq!(
            file_url(std::path::Path::new(r"C:\Mes fichiers\été & co")),
            "file:///C:/Mes%20fichiers/%C3%A9t%C3%A9%20%26%20co"
        );
    }

    #[test]
    fn shell_icons_are_valid_png() {
        let _shell = SHELL.lock().unwrap_or_else(|e| e.into_inner());
        with_com(|| {
            for key in [".txt", ".zip", "/", ""] {
                let png = type_icon_png(key).unwrap_or_else(|| panic!("pas d'icône pour {key:?}"));
                let decoder = png::Decoder::new(std::io::Cursor::new(png));
                let reader = decoder.read_info().unwrap();
                let info = reader.info();
                assert!(info.width >= 16 && info.width == info.height, "{key:?} : {}x{}", info.width, info.height);
                if let Some(dir) = std::env::var_os("WINSAR_DUMP_ICONS") {
                    let name = if key == "/" { "dossier".to_owned() } else { format!("type{key}") };
                    std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), type_icon_png(key).unwrap())
                        .unwrap();
                }
            }
        });
    }
}
