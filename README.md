<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="branding/logo/winsar-lockup-horizontal-on-dark.svg">
  <img src="branding/logo/winsar-lockup-horizontal.svg" width="300" alt="WinSAR">
</picture>

**Un gestionnaire d'archives libre pour Windows 11, écrit en Rust, qui s'intègre vraiment à l'Explorateur.**

[![CI](https://github.com/moha528/winsar/actions/workflows/ci.yml/badge.svg)](https://github.com/moha528/winsar/actions/workflows/ci.yml)
[![Licence MIT](https://img.shields.io/badge/licence-MIT-blue.svg)](LICENSE)

</div>

WinSAR remplace WinRAR et 7-Zip pour l'usage quotidien. C'est un explorateur de fichiers où une archive s'ouvre comme un dossier. Il apporte un menu contextuel natif de Windows 11, un aperçu dans le volet de l'Explorateur, et une interface Fluent avec l'effet Mica.

## Fonctionnalités

- **Formats** : ZIP, 7z, TAR, TAR.GZ, TAR.BZ2, TAR.XZ et TAR.ZST en lecture et en écriture. RAR en lecture seule, voir la [licence UnRAR](THIRD-PARTY-NOTICES.md).
- **Chiffrement AES-256** en ZIP et 7z. En 7z, les noms de fichiers sont chiffrés aussi.
- **Explorateur intégré** :
  - accueil avec accès rapide et lecteurs ;
  - barre d'adresse qui va du disque jusqu'à l'intérieur d'une archive ;
  - historique, recherche, glisser-déposer dans les deux sens.
- **Extraction intelligente** : un seul élément à la racine est déposé à côté de l'archive, sinon tout va dans un dossier à son nom. Il est aussi possible d'extraire seulement une sélection.
- **Modification d'archives** : ajout et suppression de fichiers. En ZIP, les entrées conservées sont recopiées sans recompression.
- **Conversion** d'un format à l'autre (RAR → 7z, ZIP → TAR.ZST…), l'original étant conservé.
- **Archives imbriquées** ouvertes sur place, **aperçu rapide** (Espace) des images et des textes, **sommes de contrôle** CRC32 et SHA-256.
- **Volumes** `.001`, `.002`… au format de 7-Zip, test d'intégrité, propriétés détaillées (Alt+Entrée), archives récentes.
- **Intégration Windows** :
  - menu contextuel principal de Windows 11, via `IExplorerCommand` et un package MSIX « sparse » ;
  - aperçu du contenu des archives dans le volet de visualisation, via `IPreviewHandler` ;
  - associations de fichiers ;
  - notifications natives ;
  - icônes du système, thème clair ou sombre, couleur d'accent.
- **Liste rapide** : affichage virtualisé (des dizaines de milliers de fichiers), colonnes redimensionnables, interface qui s'adapte à toutes les largeurs de fenêtre.
- **Mises à jour** : WinSAR signale les nouvelles versions publiées sur GitHub, sans rien installer sans vous.
- **CLI** : `winsar list | extract | create | convert | test`.

## Installation

Téléchargez l'installeur dans les [Releases](https://github.com/moha528/winsar/releases). Chaque version liste ses changements dans le [journal des modifications](CHANGELOG.md) et donne les sommes SHA-256 de ses fichiers.

> **Versions non signées.** WinSAR n'a pas encore de certificat de signature de code. Windows SmartScreen peut donc afficher « Windows a protégé votre ordinateur » : cliquez sur « Informations complémentaires », puis « Exécuter quand même ». Vérifiez au besoin l'installeur avec `SHA256SUMS.txt`.
>
> Pour la même raison, le menu principal de Windows 11 et l'aperçu dans l'Explorateur ne s'installent pas : l'installeur pose à la place un menu classique, dans « Afficher plus d'options » (Maj+F10). Voir [packaging/README.md](packaging/README.md).

## Compiler depuis les sources

Prérequis :

- Windows 10 21H2 ou plus récent, Windows 11 recommandé ;
- [Rust](https://rustup.rs) stable, toolchain `x86_64-pc-windows-msvc` ;
- Visual Studio Build Tools 2022 avec la charge de travail « Développement Desktop en C++ » ;
- [Node.js](https://nodejs.org) 22 ou plus récent ;
- WebView2, présent par défaut sous Windows 11.

Installer les dépendances, une seule fois :

```bash
cd app && npm install
```

L'application en développement, avec rechargement à chaud :

```bash
cd app && npm run tauri dev
```

L'installeur complet (NSIS), avec l'intégration à l'Explorateur :

```bash
cd app && npm run bundle
```

Les tests du moteur, du menu contextuel et de l'aperçu :

```bash
cargo test --workspace
```

Le CLI :

```bash
cargo run -p winsar-cli -- extract archive.7z
```

## Organisation du dépôt

| Dossier | Contenu |
|---|---|
| `crates/winsar-core` | Moteur d'archives : formats, extraction sûre, création, modification, volumes, test |
| `crates/winsar-cli` | Binaire `winsar` |
| `crates/winsar-shell` | DLL du menu contextuel de Windows 11 (`IExplorerCommand`), chargée dans `explorer.exe` |
| `crates/winsar-preview` | DLL d'aperçu dans le volet de l'Explorateur (`IPreviewHandler`) |
| `app/src-tauri` | Application Tauri 2 : commandes, intégrations Windows |
| `app/src` | Interface (TypeScript, sans framework) |
| `packaging` | Manifeste MSIX sparse, signature et installation de l'intégration à l'Explorateur |
| `branding` | Identité : logo, icônes, planche, guide d'usage ([branding/README.md](branding/README.md)) |

## Contribuer

Les contributions sont bienvenues : lisez [CONTRIBUTING.md](CONTRIBUTING.md). Le code et ses commentaires sont en français.

## Licence

WinSAR est distribué sous licence [MIT](LICENSE). L'extraction RAR repose sur le code UnRAR d'Alexander Roshal, sous sa propre licence : voir [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
