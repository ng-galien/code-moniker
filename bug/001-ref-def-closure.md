# 001 — ref/def closure violée par l'extracteur TypeScript

**Reporté** : 2026-05-08, par dogfood-import du repo ESAC sur la couche
symbolique v2 (175 fichiers TS, 15 960 refs).
**État** : ouvert.
**Composant** : `src/lang/ts/`, signature SQL publique `extract_typescript(...)`.

## Invariant violé

> Pour tout graph `g` retourné par `extract_<lang>(...)`, si `g` contient une
> ref `r` dont la cible désigne un symbole interne au corpus indexé, alors il
> existe une def `d` quelque part dans le corpus telle que
> `bind_match(d.moniker, r.target)` soit vrai.

Cas particulier resserré et entièrement testable au niveau d'un seul
extract :

> Pour tout graph `g`, si `r ∈ g.refs()` a `r.confidence ∈ {local}`, alors
> il existe `d ∈ g.defs()` avec `bind_match(d.moniker, r.target) = true`.

L'extension passe le test à l'unité sur chaque émetteur (def emit OK, ref
emit OK), mais **les deux émetteurs construisent le `name` du dernier
segment dans des dialectes différents**, donc le `bind_match` du moniker
côté ref ne peut pas matcher le moniker côté def — alors qu'ils désignent
le même symbole.

## Manifestation observée

Mesure end-to-end sur le repo `esac` ingéré via la couche symbolique v2
(`db/v2/scripts/dogfood_full_import.mjs` côté ESAC) :

| confidence | n      | résolus via `?=` | pct  |
|------------|--------|------------------|------|
| `name_match` | 12 081 | 275              | 2.3  |
| `local`    | 2 679  | **0**            | 0.0  |
| `external` | 688    | 0                | 0.0  |
| `imported` | 512    | 164              | 32.0 |
| **total**  | 15 960 | 439              | 2.8  |

`external = 0%` est attendu (cibles hors corpus). Les trois autres lignes
sont en deçà de ce que l'invariant promet ; deux causes distinctes,
toutes deux dans `src/lang/ts/`.

## Cause #1 — asymétrie de `name` entre def-emitters et ref-emitters

Trois dialectes coexistent pour le `name` du dernier segment du moniker
selon l'émetteur :

| Émetteur | Code | Format produit |
|---|---|---|
| Calls (`foo(x,y)`) | `src/lang/ts/refs.rs:396-398` `extend_callable_arity(FUNCTION, name, arity)` | `function:foo(2)` |
| Method calls (`obj.bar(x)`) | `src/lang/ts/refs.rs:400-402` `extend_callable_arity(METHOD, name, arity)` | `method:bar(1)` |
| Reads (`y = foo`) | `src/lang/ts/refs.rs:412-414` `extend_callable_typed(FUNCTION, name, [])` | `function:foo()` |
| Imports / Reexports | `src/lang/ts/imports.rs:189-192` `b.segment(PATH, name)` | `path:foo` |
| **Defs functions** | `src/lang/ts/walker.rs:551` `extend_callable_typed(FUNCTION, name, &types)` | `function:foo(int,String)` |
| **Defs methods** | `src/lang/ts/walker.rs:199` `extend_callable_typed(kind, name, &types)` | `method:bar(int)` |

`bind_match` (`src/core/moniker/...` — opérateur SQL `?=`, registered dans
`moniker_gist_ops`) ignore la **kind** du dernier segment ; il n'ignore
PAS le **name**. Donc :

- Côté def, fonction `foo(x: number)` → `function:foo(number)`
- Côté ref interne, appel `foo(42)` → `function:foo(1)` (arity=1)

`bind_match` voit `kind` différents (que c'est intentionnel, OK), mais
`name`s différents (`foo(number)` ≠ `foo(1)`) → faux. La ref ne résout
jamais sa def, alors que la def existe dans le même graphe.

Reproductible avec un test élémentaire :

```rust
#[test]
fn internal_call_target_must_bind_match_def() {
    let g = extract("util.ts",
        "function foo(x: number) {} foo(42);",
        &make_anchor(), /*deep=*/false);
    let defs: Vec<_> = g.defs().map(|d| d.moniker.clone()).collect();
    let calls: Vec<_> = g.refs().filter(|r| r.kind == b"calls").collect();
    assert_eq!(calls.len(), 1);
    assert!(
        defs.iter().any(|d| bind_match(d, &calls[0].target)),
        "calls target {} bind_matches no def in {:?}",
        calls[0].target, defs
    );
}
```

Avec le code actuel : test ÉCHOUE. `defs[1]` = `function:foo(number)`,
`calls[0].target` = `function:foo(1)`.

## Cause #2 — `deep=false` émet des refs `local` sans la def correspondante

Variante du même invariant : le walker enregistre les noms locaux dans
`Walker::local_scope` indépendamment de `deep`, donc une ref vers ce
nom est émise avec `confidence='local'` (`src/lang/ts/scope.rs:121-127`,
appelé par `refs.rs:31` et `:311`). Mais l'émission de la **def** des
locaux/params est gardée par `if self.deep` :

```
src/lang/ts/walker.rs:455-462
fn emit_param_leaf(&self, pat: Node<'_>, callable: &Moniker, graph: &mut CodeGraph) {
    for name in collect_binding_names(pat, self.source_bytes) {
        self.record_local(name.as_bytes());                  // ← inconditionnel
        if self.deep {                                       // ← gate
            let m = extend_segment(callable, kinds::PARAM, name.as_bytes());
            let _ = graph.add_def(m, kinds::PARAM, callable, Some(node_position(pat)));
        }
    }
}
```

```
src/lang/ts/walker.rs:496-512
fn handle_for_in(&self, node: Node<'_>, scope: &Moniker, graph: &mut CodeGraph) {
    if is_callable_scope(scope, &self.module) {
        ...
        self.record_local(name.as_bytes());                  // ← inconditionnel
        if self.deep {                                       // ← gate
            let m = extend_segment(scope, kinds::LOCAL, name.as_bytes());
            let _ = graph.add_def(m, kinds::LOCAL, scope, Some(node_position(c)));
        }
        ...
    }
}
```

Combiné avec la signature publique :

```
extract_typescript(
  uri text, source text, anchor moniker,
  deep boolean DEFAULT false,                       -- ← default
  di_register_callees text[] DEFAULT ARRAY[]::text[]
)
```

→ tout caller qui ne passe pas `deep=true` reçoit un graph **internement
incohérent** : N refs vers des locals, 0 def correspondante. Sur le repo
esac, 2 679 refs `confidence='local'` toutes danglantes.

L'argument "c'est intentionnel quand `deep=false`" tombe en regard de
l'invariant : la confidence `local` *affirme* que le binding est vers un
local, donc affirme l'existence du local. Émettre la ref sans la def
casse l'affirmation. Soit la ref ne devrait pas être émise quand
`deep=false` (et `confidence='local'` doit alors être impossible), soit
la def doit être émise. Le mode courant produit un graphe qui ment.

## Tests qui auraient catché le bug

Trois niveaux, par ordre d'impact :

### A. Invariant de closure intra-module (Rust unit, dans `src/lang/ts/mod.rs#tests`)

LE test à ajouter en priorité. Helper réutilisable :

```rust
#[cfg(test)]
fn assert_local_refs_closed(g: &CodeGraph) {
    let defs: Vec<_> = g.defs().map(|d| d.moniker.clone()).collect();
    for r in g.refs() {
        if r.confidence == b"local" {
            assert!(
                defs.iter().any(|d| bind_match(d, &r.target)),
                "DANGLING local ref: target={} ; no def bind_matches it.\n  Defs: {:?}",
                r.target, defs
            );
        }
    }
}
```

À appeler **en queue de chaque test d'extraction existant**. Catche
cause #2 immédiatement (avec `deep=false` les tests qui touchent un
local explosent ; force le choix conscient `deep=true` ou suppression de
l'émission de ref).

Helper équivalent pour la cause #1 :

```rust
#[cfg(test)]
fn assert_internal_call_targets_resolved(g: &CodeGraph) {
    let defs: Vec<_> = g.defs().map(|d| d.moniker.clone()).collect();
    for r in g.refs().filter(|r| r.kind == b"calls" || r.kind == b"reads") {
        // Si la ref pointe au même module que ses defs (vérifié via
        // moniker_ancestor_of contre le module root), elle DOIT résoudre.
        let same_module = defs.iter().any(|d| {
            moniker_ancestor_of(g.root(), d) && moniker_ancestor_of(g.root(), &r.target)
        });
        if same_module {
            assert!(
                defs.iter().any(|d| bind_match(d, &r.target)),
                "DANGLING internal {} ref: target={}.\n  Defs: {:?}",
                std::str::from_utf8(&r.kind).unwrap(),
                r.target, defs
            );
        }
    }
}
```

À ajouter dans les tests existants `extract_function_declaration_emits_def`
et autres dès qu'une snippet contient à la fois une déclaration et un
usage.

### B. Invariant de closure cross-module (Rust integration, dans `tests/`)

Pour le path `imports_symbol` qui n'apparaît qu'en cross-file :

```rust
#[test]
fn imported_target_bind_matches_def_in_target_module() {
    let a = extract("foo.ts",
        "export function bar(x: number) {}",
        &make_anchor(), /*deep=*/false);
    let b = extract("uses.ts",
        "import { bar } from './foo'; bar(1);",
        &make_anchor(), /*deep=*/false);

    let import_target = b.refs()
        .find(|r| r.kind == b"imports_symbol")
        .expect("import emitted")
        .target.clone();
    assert!(
        a.defs().any(|d| bind_match(&d.moniker, &import_target)),
        "imported target {} bind_matches no def in target module.\n  Defs of foo.ts: {:?}",
        import_target,
        a.defs().map(|d| d.moniker.clone()).collect::<Vec<_>>()
    );
}
```

Catche la cause #1 sur l'axe import — qui est l'usage critique en prod
puisque c'est ce qui permet la navigation cross-file.

### C. Smoke en intégration côté ESAC (pgTap, hors de cette repo)

Filet de sécurité au niveau du caller — détecte l'oubli côté ESAC de
passer `deep=true` ou les `di_register_callees`, ce que les tests Rust
ne peuvent pas voir. Documenté pour mémoire ; à instancier dans
`db/v2/04_symbol/99_smoke.sql` côté `esac` (au-delà du test d'extraction
unique déjà présent) :

```sql
SELECT pass(
  (SELECT count(*) FILTER (WHERE EXISTS (
     SELECT 1 FROM esac.module_def d
     WHERE d.plan_id = m.plan_id AND d.moniker ?= l.target_moniker
   ))::float / NULLIF(count(*),0)::float >= 0.95
   FROM esac.linkage l JOIN esac.module m ON m.id=l.source_id
   WHERE l.confidence = 'imported'),
  'imported confidence resolves >= 95% on smoke corpus'
);
```

## Pistes de fix (à arbitrer)

Pas de choix imposé — espace de design listé.

### Pour la cause #1 (asymétrie name)

Le code lu utilise actuellement 4 helpers distincts dans `src/lang/callable.rs`
et `src/lang/ts/canonicalize.rs` :

- `extend_callable_typed(parent, kind, name, &types)` → `name(t1,t2)`
- `extend_callable_arity(parent, kind, name, arity)` → `name(N)`
- `extend_segment(parent, kind, name)` → `name` nu

Trois variantes de fix :

1. **Aligner sur `extend_segment` partout** : nom nu, kind portée par le
   segment. Élimine la signature/arity du name, simplifie la closure.
   Question ouverte : comment distinguer 2 méthodes overloadées (Java
   surtout, TS rarement) ? Réponse possible : suffixe d'arité dans la
   `kind` (`method/2`) plutôt que dans le name. Reste compatible avec
   `bind_match`.

2. **Aligner sur `extend_callable_typed` partout** : signature complète
   côté def ET côté ref. Demande au resolver de calculer les types
   d'arguments à l'appel — coûteux (TS resolver), souvent impossible
   avec tree-sitter only.

3. **Étendre `bind_match`** : ignorer non seulement la `kind` du dernier
   segment mais aussi le suffixe `(...)` du name. Approche la plus
   localisée, mais perd la capacité de distinguer overloads en Java.

Au vu du SPEC.md actuel et de la philosophie "tree-sitter only, pas de
type checker", **option 1 semble la plus saine**. Mais elle casse les
tests existants qui asserent `function:foo()` côté def.

### Pour la cause #2 (deep gate)

Deux options nettes :

1. **Faire `deep=true` le default** : le graphe est cohérent par
   construction. Les callers qui veulent un graph "shallow" passent
   `deep=false` explicitement et acceptent le contrat (pas de refs
   vers locals émises non plus — voir option 2).

2. **Quand `deep=false`, ne pas émettre la ref `local` du tout** :
   conserver l'invariant en stripping les refs danglantes en sortie de
   `record_local`. Le walker conserve `local_scope` pour la
   classification de `name_confidence` (différencier locals/non-locals
   pour les autres confidences) mais n'émet pas le ref pointing into
   the void. Cohérent avec "shallow = pas de symbole local visible".

Au minimum, **documenter le contrat de `deep`** quelque part de
détectable (CLAUDE.md de l'extension, comment sur la fonction publique).

## Données brutes pour reproduction

Échantillons capturés sur le repo `esac` (commit `d25dc25`),
`extract_typescript` appelé sans `deep` ni `di_register_callees` :

```
-- local non résolu (cause #2)
TARGET : esac+moniker://esac/srcset:main/lang:ts/path:scripts/path:reingest-history/function:repoId()
SOURCE : esac+moniker://esac/srcset:main/lang:ts/path:scripts/path:reingest-history/function:reingest(string)
DEFS dans le module :
  …/path:scripts/path:reingest-history
  …/path:scripts/path:reingest-history/const:pool
  …/path:scripts/path:reingest-history/const:repos
  …/path:scripts/path:reingest-history/function:reingest(string)
→ aucun def `repoId` (variable locale dans `reingest()`)

-- import non résolu (cause #1)
TARGET : esac+moniker://esac/srcset:main/lang:ts/path:src/path:core/path:git/path:git-history/path:extractGitHistory
DEFS du module cible (git-history.ts) :
  …/function:extractGitHistory(pg.PoolClient,string,string,string,{ skipCutoff?: boolean })
→ name nu vs name signé : `extractGitHistory` ≠ `extractGitHistory(pg.PoolClient,...)`
```

## Mesure de l'impact post-fix attendu

Avec un test de stripping permissif (`name` strippé du `(...)` côté def),
la résolution `imported` passe de 32% → 73% sur le repo esac (mesuré).
Le résidu (27%) tient à des chaînes de re-export et est un autre chantier.

Pour la cause #2, mesure indirecte : 2 679 refs `local` totalement
danglantes aujourd'hui. Avec `deep=true` côté caller (ou auto-strip
côté extension), elles deviennent toutes résolues — par construction
elles pointent vers un local du même callable.
