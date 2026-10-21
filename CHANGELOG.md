# Journal des modifications

Les changements notables de Lynk. Le format suit [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/) et les versions [Semantic Versioning](https://semver.org/lang/fr/).

## [Non publié]

### Ajouté

- **Conversion d'archives** vers un autre format (RAR → 7z, ZIP → TAR.ZST…), une ou plusieurs à la fois, avec ou sans chiffrement. L'original est conservé. Aussi en ligne de commande : `lynk convert`.
- **Propriétés de l'archive** (Alt+Entrée) : format, volumes, taille, taux de compression, nombre de fichiers, chiffrement, dates, sélection.
- **Propriétés Windows** des fichiers et dossiers du disque, la feuille native de l'Explorateur.
- **Mises à jour** : Lynk signale une nouvelle version publiée sur GitHub (une fois par jour au plus, désactivable dans les paramètres).
- **Publication automatisée** : un tag `v*` fabrique l'installeur et le CLI et prépare une version en brouillon.

### Modifié

- La progression d'une modification d'archive 7z ou TAR (extraction puis recompression) tient désormais sur une seule barre.
- La taille d'une archive en volumes compte tous ses volumes.

### Corrigé

- Les fichiers chiffrés d'une archive 7z sont signalés (cadenas, propriétés), comme en ZIP et en RAR.

## [0.1.0] - 2026-10-20

Première version publique : moteur ZIP, 7z, TAR (gz, bz2, xz, zst) et RAR en lecture ; explorateur de fichiers avec archives ouvertes comme des dossiers ; menu contextuel de Windows 11 ; aperçu dans le volet de l'Explorateur ; associations de fichiers ; CLI.

[Non publié]: https://github.com/moha528/lynk-rar/compare/c54e151...HEAD
[0.1.0]: https://github.com/moha528/lynk-rar/commit/c54e151
