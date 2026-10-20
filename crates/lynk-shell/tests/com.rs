//! Charge la DLL comme le fait l'Explorateur et parcourt le menu sur une vraie sélection.
#![cfg(windows)]

use std::{ffi::c_void, path::Path};

use windows::Win32::{
    System::{
        Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, IClassFactory},
        LibraryLoader::{GetProcAddress, LoadLibraryW},
    },
    UI::Shell::{
        ECF_HASSUBCOMMANDS, ECS_ENABLED, IExplorerCommand, IShellItem, IShellItemArray, SHCreateItemFromParsingName,
        SHCreateShellItemArrayFromShellItem,
    },
};
use windows_core::{GUID, HRESULT, HSTRING, Interface, PWSTR, s};

const CLSID_LYNK: GUID = GUID::from_u128(0x556a874d_dd78_48d8_bcf4_c5cb821f6dd4);

type GetClassObject = unsafe extern "system" fn(*const GUID, *const GUID, *mut *mut c_void) -> HRESULT;

fn take(p: PWSTR) -> String {
    let s = unsafe { p.to_string() }.unwrap();
    unsafe { CoTaskMemFree(Some(p.0 as *const c_void)) };
    s
}

fn root_command() -> IExplorerCommand {
    // `cargo test` construit la DLL dans target/<profil>/deps, à côté de ce test.
    let exe = std::env::current_exe().unwrap();
    let deps = exe.parent().unwrap();
    let dll = [deps.join("lynk_shell.dll"), deps.parent().unwrap().join("lynk_shell.dll")]
        .into_iter()
        .find(|p| p.is_file())
        .expect("lynk_shell.dll introuvable");
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let module = LoadLibraryW(&HSTRING::from(dll.as_os_str())).expect("lynk_shell.dll introuvable");
        let proc = GetProcAddress(module, s!("DllGetClassObject")).expect("export manquant");
        let get: GetClassObject = std::mem::transmute(proc);
        let mut factory: Option<IClassFactory> = None;
        get(&CLSID_LYNK, &IClassFactory::IID, &mut factory as *mut _ as *mut *mut c_void).ok().unwrap();
        factory.unwrap().CreateInstance(None).unwrap()
    }
}

fn selection(path: &Path) -> IShellItemArray {
    unsafe {
        let item: IShellItem = SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None).unwrap();
        SHCreateShellItemArrayFromShellItem(&item).unwrap()
    }
}

/// Libellés visibles du sous-menu pour cette sélection.
fn visible_titles(root: &IExplorerCommand, items: &IShellItemArray) -> Vec<String> {
    let mut titles = Vec::new();
    unsafe {
        let subs = root.EnumSubCommands().unwrap();
        loop {
            let mut slot = [None];
            let mut fetched = 0;
            let _ = subs.Next(&mut slot, Some(&mut fetched));
            let Some(cmd) = slot[0].take() else { break };
            if cmd.GetState(items, true).unwrap() == ECS_ENABLED.0 as u32 {
                titles.push(cmd.GetTitle(items).map(take).unwrap_or_default());
            }
        }
    }
    titles
}

#[test]
fn archive_menu() {
    let dir = std::env::temp_dir().join(format!("lynk-shell-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let archive = dir.join("photos.tar.gz");
    std::fs::write(&archive, b"").unwrap();
    let doc = dir.join("rapport.pdf");
    std::fs::write(&doc, b"").unwrap();

    let root = root_command();
    unsafe {
        assert_eq!(take(root.GetTitle(None).unwrap()), "Lynk");
        assert_eq!(root.GetFlags().unwrap(), ECF_HASSUBCOMMANDS.0 as u32);
        assert!(take(root.GetIcon(None).unwrap()).ends_with("lynk-app.exe,0"));
        assert_eq!(root.GetState(&selection(&archive), true).unwrap(), ECS_ENABLED.0 as u32);
    }

    let titles = visible_titles(&root, &selection(&archive));
    assert_eq!(
        titles,
        [
            "Extraire ici",
            "Extraire dans « photos\\ »",
            "Extraire vers…",
            "Ouvrir avec Lynk",
            "",
            "Ajouter à « photos.zip »",
            "Ajouter à « photos.7z »",
            "Ajouter à une archive…",
        ]
    );

    // Un fichier ordinaire : seulement la compression.
    let titles = visible_titles(&root, &selection(&doc));
    assert_eq!(titles, ["Ajouter à « rapport.zip »", "Ajouter à « rapport.7z »", "Ajouter à une archive…"]);

    std::fs::remove_dir_all(dir).unwrap();
}
