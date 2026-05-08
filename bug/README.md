# bug/

Bugs documentés contre `pg_code_moniker`. Un fichier par bug ou famille de
bugs partageant le même invariant violé. Chaque entrée est numérotée + datée,
contient :

- l'invariant qu'elle viole (formulation testable)
- la manifestation observable, citée depuis un usage réel
- le code source incriminé (chemin `src/lang/...` + numéros de ligne)
- la stratégie de test qui aurait catché le bug
- les pistes de fix connues, sans choix imposé

## Index

- [001 — ref/def closure (asymétrie name + gate `deep`)](001-ref-def-closure.md) — 2026-05-08
- [002 — `read_target` / `calls_target` ignorent le scope enclosing](002-ref-target-ignores-scope.md) — 2026-05-08
- [003 — le format texte du moniker piège les consumers qui le manipulent](003-moniker-text-format-fragile-for-consumers.md) — 2026-05-08
