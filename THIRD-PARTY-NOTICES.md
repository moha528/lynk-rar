# Composants tiers

Le code de WinSAR est sous licence MIT (voir [LICENSE](LICENSE)). WinSAR s'appuie sur des bibliothèques tierces, chacune sous sa propre licence.

## UnRAR (extraction des archives RAR)

L'extraction RAR utilise le code source UnRAR d'Alexander Roshal, intégré par la crate [`unrar_sys`](https://crates.io/crates/unrar_sys). Ce code **n'est pas sous licence MIT** : il est distribué sous la licence UnRAR, qui exige de reproduire le paragraphe suivant.

> UnRAR source code may be used in any software to handle RAR archives without limitations free of charge, but cannot be used to develop RAR (WinRAR) compatible archiver and to re-create RAR compression algorithm, which is proprietary. Distribution of modified UnRAR source code in separate form or as a part of other software is permitted, provided that full text of this paragraph, starting from "UnRAR source code" words, is included in license, or in documentation if license is not available, and in source code comments of resulting package.

Conséquence pour WinSAR et ses forks : WinSAR lit les archives RAR, mais n'en crée pas, et ne doit pas en créer. Le texte complet de la licence UnRAR accompagne les sources de `unrar_sys` (`vendor/unrar/license.txt`).

## Bibliothèques Rust

Bibliothèques principales :

| Bibliothèque | Rôle | Licence |
|---|---|---|
| [tauri](https://tauri.app) | Application de bureau | MIT ou Apache-2.0 |
| [windows-rs](https://github.com/microsoft/windows-rs) | API Windows (COM, shell, GDI) | MIT ou Apache-2.0 |
| [zip](https://crates.io/crates/zip) | ZIP | MIT |
| [sevenz-rust2](https://crates.io/crates/sevenz-rust2) | 7z | Apache-2.0 |
| [tar](https://crates.io/crates/tar), [flate2](https://crates.io/crates/flate2), [bzip2](https://crates.io/crates/bzip2), [xz2](https://crates.io/crates/xz2) | TAR, gzip, bzip2, xz | MIT ou Apache-2.0 |
| [zstd](https://crates.io/crates/zstd) | Zstandard (bibliothèque C de Meta) | BSD-3-Clause : sa notice doit accompagner les binaires distribués |
| [unrar](https://crates.io/crates/unrar) | Enveloppe Rust d'UnRAR | MIT ou Apache-2.0 (code UnRAR : voir ci-dessus) |

La liste complète et exacte est celle du `Cargo.lock`. Pour l'obtenir, avec licences, installez d'abord l'outil (`cargo install cargo-license`) :

```bash
cargo license --avoid-dev-deps
```

## Interface

- [Vite](https://vitejs.dev), [TypeScript](https://www.typescriptlang.org) : outils de construction (MIT, Apache-2.0).
- Les icônes de l'interface viennent de la police **Segoe Fluent Icons** de Windows et de la liste d'images système. Elles ne sont pas redistribuées par WinSAR.
- Le logo de WinSAR (`branding/`) contient des lettres de la police **Inter** de Rasmus Andersson, converties en tracés. Licence : [SIL Open Font License 1.1](https://openfontlicense.org).
