# Journal des modifications

Les changements notables de Lynk. Le format suit [Keep a Changelog](https://keepachangelog.com/fr/1.1.0/) et les versions [Semantic Versioning](https://semver.org/lang/fr/).

## [Non publié]

## [0.1.0] - 2026-10-21

Première préversion publique.

### Archives

- ZIP, 7z, TAR, TAR.GZ, TAR.BZ2, TAR.XZ et TAR.ZST en lecture et en écriture ; RAR en lecture seule (licence UnRAR).
- Chiffrement AES-256 en ZIP et 7z (noms de fichiers compris en 7z).
- Extraction sûre (chemins assainis), intelligente (un seul élément à la racine : à côté de l'archive, sinon dans un dossier à son nom) et partielle.
- Ajout et suppression de fichiers dans une archive existante ; en ZIP, sans recompresser le reste.
- Volumes `.001`, `.002`… au format de 7-Zip, test d'intégrité.
- Conversion d'un format à l'autre (RAR → 7z, ZIP → TAR.ZST…), l'original étant conservé.
- Archives imbriquées ouvertes sur place, comme un dossier.

### Application

- Explorateur de fichiers : Accueil, accès rapide, lecteurs, archives récentes ; une archive s'ouvre comme un dossier.
- Liste virtualisée, colonnes redimensionnables, interface adaptée aux fenêtres étroites, thème clair ou sombre, effet Mica.
- Aperçu rapide (Espace) des images et des textes ; propriétés détaillées d'une archive (Alt+Entrée) et feuille de propriétés de Windows pour les fichiers.
- Sommes de contrôle CRC32 et SHA-256, avec vérification d'une somme collée.
- « Nouvelle archive » : format, niveau, mot de passe, volumes, une archive par élément.
- Glisser-déposer dans les deux sens, icônes du système, notifications.
- Paramètres : démarrage, fichiers cachés, extraction, compression, associations de fichiers, entrées du menu contextuel, mises à jour.
- Annonce des nouvelles versions publiées sur GitHub (une fois par jour au plus, désactivable).

### Intégration à Windows

- Menu contextuel principal de Windows 11 (`IExplorerCommand`, package MSIX « sparse »).
- Menu contextuel classique de repli, posé par l'installeur quand le package n'a pas pu être enregistré.
- Aperçu du contenu des archives dans le volet de visualisation de l'Explorateur.
- Installeur NSIS par utilisateur, sans droits d'administrateur.

### Outils

- CLI `lynk` : `list`, `extract`, `create`, `convert`, `test`, `formats`.
- Publication automatisée : un tag `v*` fabrique l'installeur et le CLI et prépare une version en brouillon.

[Non publié]: https://github.com/moha528/lynk-rar/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/moha528/lynk-rar/releases/tag/v0.1.0
