//! Pilote le gestionnaire d'aperçu comme le fait prevhost.exe.
#![cfg(windows)]

use std::{ffi::c_void, path::Path};

use windows::Win32::{
    Foundation::{COLORREF, HWND, LPARAM, RECT, WPARAM},
    System::{
        Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, IClassFactory},
        LibraryLoader::{GetProcAddress, LoadLibraryW},
    },
    UI::{
        Controls::{LVM_GETITEMCOUNT, WC_LISTVIEWW},
        Shell::{IPreviewHandler, IPreviewHandlerVisuals, PropertiesSystem::IInitializeWithFile},
        WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, FindWindowExW, IsWindow, SendMessageW, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW,
        },
    },
};
use windows_core::{GUID, HRESULT, HSTRING, Interface, PCWSTR, s, w};
use winsar_core::{CreateOptions, Format};

const CLSID: GUID = GUID::from_u128(0x91de3b61_5509_4a65_be33_26284df4895c);

type GetClassObject = unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;

fn new_handler() -> IPreviewHandler {
    let exe = std::env::current_exe().unwrap();
    // `cargo test` construit la DLL dans target/<profil>/deps, à côté de ce test.
    let deps = exe.parent().unwrap();
    let dll = [deps.join("winsar_preview.dll"), deps.parent().unwrap().join("winsar_preview.dll")]
        .into_iter()
        .find(|p| p.is_file())
        .expect("winsar_preview.dll introuvable");
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let module = LoadLibraryW(&HSTRING::from(dll.as_os_str())).expect("winsar_preview.dll introuvable");
        let get: GetClassObject = std::mem::transmute(GetProcAddress(module, s!("DllGetClassObject")).unwrap());
        let mut factory: Option<IClassFactory> = None;
        get(&CLSID, &IClassFactory::IID, &mut factory as *mut _ as *mut *mut c_void).ok().unwrap();
        factory.unwrap().CreateInstance(None).unwrap()
    }
}

/// Aperçu de `archive` dans une fenêtre hôte ; rend le nombre de lignes affichées.
fn preview_rows(archive: &Path) -> isize {
    unsafe {
        let host = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("STATIC"),
            w!(""),
            WS_OVERLAPPEDWINDOW,
            0,
            0,
            640,
            400,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        let handler = new_handler();
        let path = HSTRING::from(archive.as_os_str());
        handler.cast::<IInitializeWithFile>().unwrap().Initialize(&path, 0).unwrap();
        handler.SetWindow(host, &RECT { left: 0, top: 0, right: 640, bottom: 400 }).unwrap();
        handler.cast::<IPreviewHandlerVisuals>().unwrap().SetBackgroundColor(COLORREF(0x00202020)).unwrap();
        handler.DoPreview().unwrap();

        let list: HWND = FindWindowExW(Some(host), None, WC_LISTVIEWW, PCWSTR::null()).expect("liste absente");
        let rows = SendMessageW(list, LVM_GETITEMCOUNT, Some(WPARAM(0)), Some(LPARAM(0))).0;
        handler.SetRect(&RECT { left: 0, top: 0, right: 300, bottom: 200 }).unwrap();
        handler.Unload().unwrap();
        assert!(!IsWindow(Some(list)).as_bool(), "la liste doit disparaître au déchargement");
        let _ = DestroyWindow(host);
        rows
    }
}

fn sample(dir: &Path) -> std::path::PathBuf {
    let src = dir.join("projet");
    std::fs::create_dir_all(src.join("docs")).unwrap();
    std::fs::write(src.join("a.txt"), "a").unwrap();
    std::fs::write(src.join("docs/b.pdf"), "b").unwrap();
    src
}

#[test]
fn lists_archive_entries() {
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path());
    let zip = tmp.path().join("p.zip");
    winsar_core::create(&[src], &zip, &CreateOptions::default(), &winsar_core::NoProgress).unwrap();
    let expected = winsar_core::list(&zip, None).unwrap().len() as isize;
    assert_eq!(preview_rows(&zip), expected);
}

#[test]
fn encrypted_archive_shows_a_single_notice() {
    let tmp = tempfile::tempdir().unwrap();
    let src = sample(tmp.path());
    let archive = tmp.path().join("s.7z");
    let opts = CreateOptions { format: Format::SevenZ, password: Some("x".into()), ..Default::default() };
    winsar_core::create(&[src], &archive, &opts, &winsar_core::NoProgress).unwrap();
    assert_eq!(preview_rows(&archive), 1);
}
