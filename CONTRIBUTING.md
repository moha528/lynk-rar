# Contribuer à Lynk

Merci de votre intérêt ! Le guide tient en une page.

## Avant de commencer

- Pour un changement important, ouvrez d'abord une issue pour en discuter.
- Pour un bug, donnez la version de Windows, le format d'archive concerné et, si possible, une archive qui le reproduit.

## Mettre en place l'environnement

Les prérequis sont dans le [README](README.md#compiler-depuis-les-sources). Ensuite :

```bash
cd app && npm install && npm run tauri dev
```

Pour travailler l'interface sans compiler Rust, une page d'aperçu simule le côté Rust ([src/dev/mock.ts](app/src/dev/mock.ts)) ; ouvrez ensuite `http://localhost:5173/preview.html` :

```bash
cd app && npx vite
```

Sur une machine où Smart App Control est actif, Windows bloque les exécutables que compile Cargo. Désactivez-le pour développer.

## Avant d'ouvrir une pull request

La CI vérifie la même chose que ces commandes ; lancez-les depuis la racine du dépôt :

```bash
cargo fmt --all --check
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

```bash
cargo test --workspace
```

```bash
cd app && npm run typecheck
```

## Conventions

- **Langue** : les commentaires, les messages et l'interface sont en français.
- **Commentaires** : ils expliquent le *pourquoi* (une contrainte de Windows, un piège de format), pas ce que dit déjà le code.
- **Sécurité** :
  - un chemin qui vient d'une archive passe toujours par `lynk_core::safe_relative` ;
  - aucune écriture partielle ne doit rester en cas d'erreur ou d'annulation.
- **Extensions de l'Explorateur** :
  - `lynk-shell` tourne dans `explorer.exe` : pas de dépendance lourde, pas de `panic!` (tout passe par `guard`), et aucun `panic = "abort"` dans le profil `release` ;
  - `lynk-preview` tourne dans `prevhost.exe`.
- **RAR** : Lynk n'en crée pas et ne doit pas en créer (licence UnRAR, voir [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)).
- **Interface** : TypeScript sans framework, styles Fluent (`app/src/styles.css`), thèmes clair et sombre à vérifier tous les deux, ainsi qu'une fenêtre étroite.

## Licence des contributions

En proposant une contribution, vous acceptez qu'elle soit distribuée sous la [licence MIT](LICENSE) du projet.
