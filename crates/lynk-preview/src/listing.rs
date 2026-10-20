//! La liste native (ListView en mode détails) qui affiche le contenu de l'archive.

use std::{collections::HashMap, mem::size_of, path::Path};

use chrono::{Local, TimeZone};
use lynk_core::{Entry, Error};
use windows::Win32::{
    Foundation::{COLORREF, HWND, LPARAM, RECT, WPARAM},
    Storage::FileSystem::{FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL},
    UI::{
        Controls::{
            ICC_LISTVIEW_CLASSES, INITCOMMONCONTROLSEX, InitCommonControlsEx, LVCF_FMT, LVCF_TEXT, LVCF_WIDTH,
            LVCFMT_LEFT, LVCFMT_RIGHT, LVCOLUMNW, LVIF_IMAGE, LVIF_TEXT, LVITEMW, LVM_DELETEALLITEMS,
            LVM_INSERTCOLUMNW, LVM_INSERTITEMW, LVM_SETBKCOLOR, LVM_SETCOLUMNWIDTH, LVM_SETEXTENDEDLISTVIEWSTYLE,
            LVM_SETIMAGELIST, LVM_SETITEMTEXTW, LVM_SETTEXTBKCOLOR, LVM_SETTEXTCOLOR, LVS_EX_DOUBLEBUFFER,
            LVS_EX_FULLROWSELECT, LVS_NOSORTHEADER, LVS_REPORT, LVS_SHAREIMAGELISTS, LVS_SINGLESEL, LVSIL_SMALL,
            SetWindowTheme, WC_LISTVIEWW,
        },
        Shell::{SHFILEINFOW, SHGFI_SMALLICON, SHGFI_SYSICONINDEX, SHGFI_USEFILEATTRIBUTES, SHGetFileInfoW},
        WindowsAndMessaging::{
            CreateWindowExW, SendMessageW, WINDOW_EX_STYLE, WINDOW_STYLE, WS_CHILD, WS_CLIPSIBLINGS, WS_VISIBLE,
        },
    },
};
use windows_core::{HSTRING, PWSTR, Result, w};

/// Au-delà, l'aperçu reste lisible et instantané ; le reste est résumé.
const MAX_ROWS: usize = 5000;
const SIZE_COL: i32 = 90;
const DATE_COL: i32 = 120;

pub fn create(parent: HWND, r: &RECT) -> Result<HWND> {
    unsafe {
        let icc =
            INITCOMMONCONTROLSEX { dwSize: size_of::<INITCOMMONCONTROLSEX>() as u32, dwICC: ICC_LISTVIEW_CLASSES };
        let _ = InitCommonControlsEx(&icc);
        // LVS_SHAREIMAGELISTS : la liste d'icônes est celle du système, à ne jamais détruire.
        let style = WS_CHILD
            | WS_VISIBLE
            | WS_CLIPSIBLINGS
            | WINDOW_STYLE(LVS_REPORT | LVS_SHAREIMAGELISTS | LVS_SINGLESEL | LVS_NOSORTHEADER);
        let list = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            WC_LISTVIEWW,
            w!(""),
            style,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            Some(parent),
            None,
            None,
            None,
        )?;
        let ex = LVS_EX_FULLROWSELECT | LVS_EX_DOUBLEBUFFER;
        send(list, LVM_SETEXTENDEDLISTVIEWSTYLE, ex as usize, ex as isize);
        add_column(list, 0, "Nom", (r.right - r.left - SIZE_COL - DATE_COL).max(120), false);
        add_column(list, 1, "Taille", SIZE_COL, true);
        add_column(list, 2, "Modifié le", DATE_COL, false);
        Ok(list)
    }
}

fn send(hwnd: HWND, msg: u32, w: usize, l: isize) -> isize {
    unsafe { SendMessageW(hwnd, msg, Some(WPARAM(w)), Some(LPARAM(l))).0 }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn add_column(list: HWND, index: usize, title: &str, width: i32, right: bool) {
    let mut text = wide(title);
    let col = LVCOLUMNW {
        mask: LVCF_TEXT | LVCF_WIDTH | LVCF_FMT,
        fmt: if right { LVCFMT_RIGHT } else { LVCFMT_LEFT },
        cx: width,
        pszText: PWSTR(text.as_mut_ptr()),
        ..Default::default()
    };
    send(list, LVM_INSERTCOLUMNW, index, &col as *const _ as isize);
}

/// La colonne « Nom » prend toute la largeur restante.
pub fn fit_columns(list: HWND, width: i32) {
    let name = (width - SIZE_COL - DATE_COL - 24).max(120);
    send(list, LVM_SETCOLUMNWIDTH, 0, name as isize);
}

/// Couleurs fournies par l'Explorateur (thème clair ou sombre).
pub fn apply_colors(list: HWND, background: Option<COLORREF>, text: Option<COLORREF>) {
    if let Some(bg) = background {
        send(list, LVM_SETBKCOLOR, 0, bg.0 as isize);
        send(list, LVM_SETTEXTBKCOLOR, 0, bg.0 as isize);
        // Fond sombre : barres de défilement et en-têtes sombres, comme l'Explorateur.
        let (r, g, b) = (bg.0 & 0xFF, (bg.0 >> 8) & 0xFF, (bg.0 >> 16) & 0xFF);
        let dark = (r * 299 + g * 587 + b * 114) / 1000 < 128;
        let theme = if dark { w!("DarkMode_Explorer") } else { w!("Explorer") };
        unsafe {
            let _ = SetWindowTheme(list, theme, None);
        }
    }
    if let Some(fg) = text {
        send(list, LVM_SETTEXTCOLOR, 0, fg.0 as isize);
    }
}

/// Index des icônes dans la liste d'images du système, par type de fichier.
struct Icons {
    cache: HashMap<String, i32>,
}

impl Icons {
    fn index(&mut self, list: HWND, name: &str, is_dir: bool) -> i32 {
        let key = if is_dir {
            "/".to_owned()
        } else {
            name.rfind('.').filter(|&i| i > 0).map_or(String::new(), |i| name[i..].to_ascii_lowercase())
        };
        if let Some(&i) = self.cache.get(&key) {
            return i;
        }
        let mut info = SHFILEINFOW::default();
        let attrs = if is_dir { FILE_ATTRIBUTE_DIRECTORY } else { FILE_ATTRIBUTE_NORMAL };
        let probe = if is_dir { "dossier".to_owned() } else { format!("fichier{key}") };
        let himl = unsafe {
            SHGetFileInfoW(
                &HSTRING::from(probe),
                attrs,
                Some(&mut info),
                size_of::<SHFILEINFOW>() as u32,
                SHGFI_SYSICONINDEX | SHGFI_SMALLICON | SHGFI_USEFILEATTRIBUTES,
            )
        };
        if himl != 0 && self.cache.is_empty() {
            send(list, LVM_SETIMAGELIST, LVSIL_SMALL as usize, himl as isize);
        }
        self.cache.insert(key, info.iIcon);
        info.iIcon
    }
}

fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["o", "Ko", "Mo", "Go", "To"];
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} o") } else { format!("{v:.1} {}", UNITS[unit]).replace('.', ",") }
}

fn insert(list: HWND, row: usize, icons: &mut Icons, name: &str, is_dir: bool, size: &str, date: &str) {
    let mut text = wide(name);
    let item = LVITEMW {
        mask: LVIF_TEXT | LVIF_IMAGE,
        iItem: row as i32,
        pszText: PWSTR(text.as_mut_ptr()),
        iImage: icons.index(list, name, is_dir),
        ..Default::default()
    };
    let index = send(list, LVM_INSERTITEMW, 0, &item as *const _ as isize);
    if index < 0 {
        return;
    }
    for (sub, value) in [(1, size), (2, date)] {
        if value.is_empty() {
            continue;
        }
        let mut text = wide(value);
        let item = LVITEMW { iSubItem: sub, pszText: PWSTR(text.as_mut_ptr()), ..Default::default() };
        send(list, LVM_SETITEMTEXTW, index as usize, &item as *const _ as isize);
    }
}

/// Lignes à afficher, par chemin complet sans tenir compte de la casse :
/// chaque dossier précède ainsi son contenu.
pub fn rows(mut entries: Vec<Entry>) -> Vec<Entry> {
    entries.sort_by_cached_key(|e| e.path.to_lowercase());
    entries
}

pub fn fill(list: HWND, path: &Path) {
    send(list, LVM_DELETEALLITEMS, 0, 0);
    let mut icons = Icons { cache: HashMap::new() };
    let entries = match lynk_core::list(path, None) {
        Ok(entries) => rows(entries),
        Err(Error::Password) => {
            insert(list, 0, &mut icons, "Contenu protégé par un mot de passe", false, "", "");
            return;
        }
        Err(e) => {
            insert(list, 0, &mut icons, &format!("Aperçu impossible : {e}"), false, "", "");
            return;
        }
    };
    for (row, e) in entries.iter().take(MAX_ROWS).enumerate() {
        let date = e
            .modified
            .and_then(|t| Local.timestamp_opt(t, 0).single())
            .map(|d| d.format("%d/%m/%Y %H:%M").to_string())
            .unwrap_or_default();
        let size = if e.is_dir { String::new() } else { human(e.size) };
        insert(list, row, &mut icons, &e.path, e.is_dir, &size, &date);
    }
    if entries.len() > MAX_ROWS {
        let more = format!("… et {} autres éléments", entries.len() - MAX_ROWS);
        insert(list, MAX_ROWS, &mut icons, &more, false, "", "");
    }
}
