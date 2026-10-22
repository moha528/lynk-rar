# Intégration à l'Explorateur

WinSAR s'ajoute au menu contextuel **principal** de Windows 11, pas dans « Afficher plus d'options ». Il affiche aussi le contenu des archives dans le **volet de visualisation**, activé par Alt+P dans l'Explorateur. Il faut pour cela :

- `winsar_shell.dll` (`crates/winsar-shell`), qui implémente `IExplorerCommand` : l'entrée « WinSAR » et son sous-menu, chargés dans `explorer.exe` ;
- `winsar_preview.dll` (`crates/winsar-preview`), qui implémente `IPreviewHandler` : une liste native avec les icônes du système, chargée dans `prevhost.exe` ;
- un package MSIX « sparse » signé ([AppxManifest.xml](AppxManifest.xml)), qui donne à WinSAR une identité de package et déclare la DLL auprès de l'Explorateur. Les binaires ne sont pas copiés dans le package : ils restent dans `target\release`.

## Installeur complet (NSIS)

```powershell
cd app; npm run bundle
```

`npm run bundle` lance `tauri build` avec [tauri.bundle.conf.json](../app/src-tauri/tauri.bundle.conf.json). Avant de compiler l'application, [build-msix.ps1](build-msix.ps1) compile `winsar_shell.dll` en release et produit `target\WinSAR.msix`, signé. Les deux fichiers sont ensuite embarqués dans l'installeur, et les [crochets NSIS](../app/src-tauri/windows/hooks.nsh) :

- **à l'installation** : enregistrent le package, en pointant vers le dossier d'installation ;
- **à la mise à jour** : le retirent d'abord, puis renomment la DLL, que l'Explorateur peut encore tenir chargée ;
- **à la désinstallation** : le retirent.

L'installeur produit se trouve dans `target\release\bundle\nsis\`.

Le manifeste n'a pas à être retouché à la main : au moment de l'empaquetage, sa version est celle du workspace (`Cargo.toml`) et son `Publisher` le sujet du certificat qui signe.

**Diffusion.** Le certificat `CN=WinSAR Dev` n'est approuvé que sur votre machine. Sur un autre poste, WinSAR s'installe et fonctionne, mais sans menu contextuel. Pour diffuser :

1. obtenez un certificat de signature de code reconnu, par exemple via Azure Trusted Signing ;
2. en local, définissez `WINSAR_CERT_THUMBPRINT` avec son empreinte avant `npm run bundle` ;
3. pour les versions fabriquées par GitHub Actions ([release.yml](../.github/workflows/release.yml)), ajoutez les secrets `WINSAR_CERT_PFX` (le `.pfx` en base64) et `WINSAR_CERT_PASSWORD` au dépôt. Sans eux, la CI signe avec un certificat jetable.

## Menu principal sur votre PC avec une version non signée

Une version publiée par GitHub Actions sans certificat reconnu n'obtient que le menu classique (« Afficher plus d'options »). Pour avoir l'entrée WinSAR dans le menu principal sur **votre** machine, resignez le package avec le certificat de développement et approuvez-le, ici seulement (une fenêtre UAC) :

```powershell
powershell -ExecutionPolicy Bypass -File packaging\install-shell.ps1 -InstallDir "$env:LOCALAPPDATA\WinSAR" -RestartExplorer
```

Il faut le SDK Windows (makeappx, signtool). Le script retire ensuite le menu classique, qui ferait doublon. À refaire après chaque mise à jour de WinSAR : l'installeur réenregistre le package non signé.

## Installation de développement (sans installeur)

```powershell
cd app; npm run tauri build -- --no-bundle; cd ..   # target\release\winsar-app.exe
cargo build --release -p winsar-shell -p winsar-preview   # les deux DLL
powershell -ExecutionPolicy Bypass -File packaging\install-shell.ps1 -RestartExplorer
```

La première fois, le script crée un certificat auto-signé `CN=WinSAR Dev` et demande, par une fenêtre UAC, de l'approuver dans les « Personnes autorisées » de la machine. Les fois suivantes, il se contente de réempaqueter et de réenregistrer.

`-Configuration debug` pointe vers `target\debug`. Le `winsar-app.exe` de debug charge alors l'interface depuis le serveur Vite : `npm run tauri dev` doit tourner.

## Mettre à jour la DLL

L'Explorateur garde la DLL chargée tant qu'il s'en sert. Après une recompilation, relancez le script avec `-RestartExplorer`.

## Désinstaller

```powershell
powershell -ExecutionPolicy Bypass -File packaging\install-shell.ps1 -Uninstall -RestartExplorer
```

Le certificat reste dans les magasins. Pour le retirer : `certmgr.msc` → Personnes autorisées, ainsi que `certmgr.msc` → Personnel.

## Contrat avec l'application

La DLL lance `winsar-app.exe`, posé à côté d'elle, avec l'un de ces arguments :

| Entrée du menu                            | Arguments                         |
|-------------------------------------------|-----------------------------------|
| Extraire ici                              | `--extract-here <archives…>`      |
| Extraire dans « nom\ »                    | `--extract-named <archives…>`     |
| Extraire vers…                            | `--extract-to <archives…>`        |
| Ouvrir avec WinSAR                          | `<archive>`                       |
| Ajouter à « nom.zip » / « nom.7z »        | `--compress zip\|7z <éléments…>`  |
| Ajouter à une archive…                    | `--compress-dialog <éléments…>`   |

Au-delà d'environ 24 000 caractères, les chemins passent par `--files-from <fichier temporaire>`, un chemin par ligne, que `winsar-app` supprime après lecture.
