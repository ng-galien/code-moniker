# CLAUDE.md

`pg_code_moniker` — extension PostgreSQL Rust + pgrx. Types natifs `moniker` et `code_graph` avec algèbre indexée GiST. Pas de tables, pas de triggers, pas d'état persistant — **types + opérateurs + extracteurs par langage**.

Premier consommateur : ESAC. **La boussole de toute décision : améliorer l'expérience symbolique d'ESAC** (`esac_symbol` find/refs/carriers/families/health/gaps, `esac_outline`), jamais la détériorer. Chaque ligne ajoutée doit être traçable à une opération de cette chorégraphie. Si une feature ne sert pas une de ces actions, elle n'a pas sa place dans l'extension.

## Documents

- `README.md` — posture, scope, build/test commands
- `SPEC.md` — modèle conceptuel (canonical tree, moniker, code_graph, srcset, trois origines), API publique, format URI SCIP, phases d'implémentation
- `CLAUDE.md` (ce fichier) — règles de codage et état d'avancement

Pas d'archive de chantier, pas de mémo de décision, pas de doc spéculatif. Git log + le code + ces trois fichiers sont la source de vérité.

## Sobriété des commentaires

- **Code** : commentaires minimaux. Pas de narration de ce que le code fait — le nom des items et le flot le disent. Pas de roman dans les `//!` de module : un paragraphe court suffit.
- **Tests** : c'est la place légitime de la documentation. Une description courte de l'invariant testé est bienvenue. Le nom du test est la spec (`extract_simple_class_emits_class_def`).
- Pas d'emoji. Pas de framing « smart ». Sobre, technique.

## Layout

```
src/
  lib.rs              entry, gates pgrx behind pgN features
  core/               pure Rust, no pgrx, testable with cargo test
    kind_registry.rs  KindId + PunctClass (Path/Type/Term/Method)
    moniker.rs        bytea encoding + builder + view + iterator
    uri.rs            SCIP parse / serialize, backtick escaping
    code_graph.rs     defs / refs / tree per module
  pg/                 pgrx wrappers, gated behind pgN feature
  lang/               per-language extractors
    mod.rs
    ts/               cible : un sous-dossier par langage
      mod.rs          pub fn parse, pub fn extract
      walker.rs       AST traversal
      canonicalize.rs moniker construction from AST nodes
      refs.rs         refs extraction (imports, calls, extends, ...)
      kinds.rs        language-specific kind interning
    java/             future
    python/           future
    pgsql/            future
tests/fixtures/<lang>/   source fixtures with expected code_graph snapshots
```

Aujourd'hui `lang/ts.rs` est monolithique (~280 lignes) ; à splitter selon ce layout dès qu'il dépasse ~400 lignes. **Pas de fichier > ~600 lignes.** Une responsabilité par fichier, nommée par son suffixe.

## TDD

Tests décrivent le contrat avant l'implémentation. Cycle : test rouge → impl minimale → vert → cycle suivant. Tests inline dans `#[cfg(test)] mod tests` à côté du code testé — convention Rust standard, accès aux items privés sans cérémonie. Quand un fichier dépasse le cap, splitter le module de production (sous-fichiers avec leurs propres `mod tests`), pas extraire les tests. `cargo test` pour `core/` et `lang/` (pure Rust, pas de PG) ; `cargo pgrx test pgN` pour la couche `pg/` (en-PG, behind feature `pg_test`).

