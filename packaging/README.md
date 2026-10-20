# Intégration à l'Explorateur

Lynk s'ajoute au menu contextuel **principal** de Windows 11, pas dans « Afficher plus d'options ». Il affiche aussi le contenu des archives dans le **volet de visualisation**, activé par Alt+P dans l'Explorateur. Il faut pour cela :

- `lynk_shell.dll` (`crates/lynk-shell`), qui implémente `IExplorerCommand` : l'entrée « Lynk » et son sous-menu, chargés dans `explorer.exe` ;
- `lynk_preview.dll` (`crates/lynk-preview`), qui implémente `IPreviewHandler` : une liste native avec les icônes du système, chargée dans `prevhost.exe` ;
- un package MSIX « sparse » signé ([AppxManifest.xml](AppxManifest.xml)), qui donne à Lynk une identité de package et déclare la DLL auprès de l'Explorateur. Les binaires ne sont pas copiés dans le package : ils restent dans `target\release`.

## Installeur complet (NSIS)

```powershell
cd app; npm run bundle
```

`npm run bundle` lance `tauri build` avec [tauri.bundle.conf.json](../app/src-tauri/tauri.bundle.conf.json). Avant de compiler l'application, [build-msix.ps1](build-msix.ps1) compile `lynk_shell.dll` en release et produit `target\Lynk.msix`, signé. Les deux fichiers sont ensuite embarqués dans l'installeur, et les [crochets NSIS](../app/src-tauri/windows/hooks.nsh) :

- **à l'installation** : enregistrent le package, en pointant vers le dossier d'installation ;
- **à la mise à jour** : le retirent d'abord, puis renomment la DLL, que l'Explorateur peut encore tenir chargée ;
- **à la désinstallation** : le retirent.

L'installeur produit se trouve dans `target\release\bundle\nsis\`.

**Diffusion.** Le certificat `CN=Lynk Dev` n'est approuvé que sur votre machine. Sur un autre poste, Lynk s'installe et fonctionne, mais sans menu contextuel. Pour diffuser :

1. obtenez un certificat de signature de code reconnu, par exemple via Azure Trusted Signing ;
2. remplacez `Publisher` dans [AppxManifest.xml](AppxManifest.xml) par le sujet exact de ce certificat ;
3. définissez `LYNK_CERT_THUMBPRINT` avec son empreinte avant `npm run bundle`.

## Installation de développement (sans installeur)

```powershell
cd app; npm run tauri build -- --no-bundle; cd ..   # target\release\lynk-app.exe
cargo build --release -p lynk-shell -p lynk-preview   # les deux DLL
powershell -ExecutionPolicy Bypass -File packaging\install-shell.ps1 -RestartExplorer
```

La première fois, le script crée un certificat auto-signé `CN=Lynk Dev` et demande, par une fenêtre UAC, de l'approuver dans les « Personnes autorisées » de la machine. Les fois suivantes, il se contente de réempaqueter et de réenregistrer.

`-Configuration debug` pointe vers `target\debug`. Le `lynk-app.exe` de debug charge alors l'interface depuis le serveur Vite : `npm run tauri dev` doit tourner.

## Mettre à jour la DLL

L'Explorateur garde la DLL chargée tant qu'il s'en sert. Après une recompilation, relancez le script avec `-RestartExplorer`.

## Désinstaller

```powershell
powershell -ExecutionPolicy Bypass -File packaging\install-shell.ps1 -Uninstall -RestartExplorer
```

Le certificat reste dans les magasins. Pour le retirer : `certmgr.msc` → Personnes autorisées, ainsi que `certmgr.msc` → Personnel.

## Contrat avec l'application

La DLL lance `lynk-app.exe`, posé à côté d'elle, avec l'un de ces arguments :

| Entrée du menu                            | Arguments                         |
|-------------------------------------------|-----------------------------------|
| Extraire ici                              | `--extract-here <archives…>`      |
| Extraire dans « nom\ »                    | `--extract-named <archives…>`     |
| Extraire vers…                            | `--extract-to <archives…>`        |
| Ouvrir avec Lynk                          | `<archive>`                       |
| Ajouter à « nom.zip » / « nom.7z »        | `--compress zip\|7z <éléments…>`  |
| Ajouter à une archive…                    | `--compress-dialog <éléments…>`   |

Au-delà d'environ 24 000 caractères, les chemins passent par `--files-from <fichier temporaire>`, un chemin par ligne, que `lynk-app` supprime après lecture.
