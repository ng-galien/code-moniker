# Préparation de Code Moniker 0.13.0

État au 25 septembre 2026 : candidat préparé et validé localement sur macOS
arm64. Aucun push, tag ni publication effectué par cette préparation.

## Périmètre

- Sept crates Rust, binaires CLI/MCP, `@code-moniker/client` et ses quatre
  paquets natifs alignés sur `0.13.0`.
- Changements SQL : commentaires de colonnes, signatures des contraintes,
  identités stables des attributs, normalisation des types et traitement des
  ajouts/remplacements/suppressions dans un même fichier.
- Diff impact : signatures et identités des sources des références exposées
  dans le contrat public. Protocole daemon passé de 23 à 24 ; les clients
  doivent utiliser le protocole correspondant.
- City Map conservé dans un commit distinct, `bae36e0f`, avec `private: true`.
  Aucun ajout aux publications npm ou cargo-dist. Les captures locales ne
  sont pas committées. La présence des sources dans Git ne publie pas le module.
- L'extension VS Code conserve sa publication indépendante. Une extension
  utilisant le protocole 23 n'est pas compatible avec ce nouveau daemon.

## Validations locales effectuées

- `cargo fmt --all -- --check` : réussi.
- `cargo clippy --workspace --all-targets --no-deps -- -D warnings` : réussi.
- `cargo test --workspace --quiet --no-fail-fast -- --skip git_metadata_plan_resolves_linked_worktree_private_and_common_dirs --test-threads=1` : réussi hors sandbox.
  Le test exclu crée un worktree Git, interdit par les instructions du dépôt.
  Un autre test reste ignoré par la suite existante.
- Contrôle d'architecture avec le binaire local 0.13.0,
  `check . --profile ci --report` : zéro violation sur 972 fichiers analysés.
  Règles existantes conservées, sans suppression ni affaiblissement.
- Export Rust du schéma daemon : identique octet pour octet au fichier suivi,
  protocole 24. Types et constante TypeScript régénérés.
- Client : installation propre `npm ci --omit=optional`, puis `npm test`
  réussi ; 34 tests unitaires, 2 tests de paquet, trois modes de compilation
  des consommateurs TypeScript et métadonnées des quatre paquets natifs.
  Trois tests spécifiques à Windows ne s'exécutent pas sur macOS.
- Tests Node avec le daemon local : diff impact, lancement/arrêt à froid et
  à chaud, mutation après indexation, règles SQL et installation des archives
  npm client + darwin-arm64 réussis. Fixture : 512 fichiers, 16 896 symboles,
  8 192 références avant mutation. Binaire de développement, pas artefact de CI.
- City Map : compilation et 14 tests réussis ; aucun travail graphique ajouté
  pendant cette préparation de release.
- `dist plan --tag=v0.13.0` : cinq cibles attendues, installateur, sources et
  checksums ; application unique `code-moniker`, aucune distribution City Map.
- `code-moniker mcp --help` : disponible dans le binaire construit.

Les premières passes ont révélé des attendus SQL devenus obsolètes, actualisés
après examen. Des tests de surveillance ont échoué dans la sandbox puis passé
hors sandbox ; des contrôles Git chronométrés ont échoué sous charge puis passé
sans compilation concurrente. Aucun délai ni assertion de production n'a été
affaibli pour obtenir ces résultats. Le test de documentation embarquée a été
rejoué avec succès après recompilation suivant sa dernière modification.

## Avant publication

Après autorisation de push, attendre les quatre jobs de CI de release sur le
commit candidat, notamment Windows et Linux statique. Les cinq binaires de
release, leur provenance et leur installation multiplateforme restent à
valider par la chaîne de distribution décrite dans `docs/release.md`.

Ne créer/pousser le tag `v0.13.0` qu'après validation et autorisation de publier.
Le tag déclenche les publications crates.io et npm ; il ne constitue pas une
simple sauvegarde. City Map doit rester privé et hors de ces publications.
