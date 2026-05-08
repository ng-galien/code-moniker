# 002 — `read_target` / `calls_target` ignorent le scope enclosing

**Reporté** : 2026-05-08, mesuré post-fix-side-ESAC du bug 001.
**État** : ouvert.
**Composant** : `src/lang/ts/refs.rs`.

## Invariant violé

> Pour toute ref `r` avec `confidence='local'`, le moniker `r.target` doit
> appartenir au sous-arbre du scope enclosing du site de la ref. C'est ce
> qui fait sens du mot "local" — la cible vit dans le même callable.

L'extension émet le ref-target depuis `self.module` (la racine du module),
pas depuis le scope passé en paramètre. Donc une lecture de `m` à
l'intérieur de `main()` produit un target sous `module/function:m`
au lieu de `module/function:main()/local:m`. La cible et la def n'ont pas
le même path — `bind_match` ne peut pas les rapprocher.

Distinct du bug 001 :
- 001 = asymétrie de **name** (parens vs nu) au sein d'un même path,
  partiellement adressable par strip côté caller.
- 002 = asymétrie de **path** (module-rooted vs scope-rooted), non
  adressable par strip — il faudrait remonter / parcourir l'arbre des
  scopes côté caller pour résoudre.

## Manifestation observée

Mesure post-fix côté ESAC (workaround strip + `deep=true` activés) sur
le repo `esac` (108 modules extraits, 68 fichiers KO sur bug 003) :

| confidence | n     | strict | strict% | stripped | stripped% |
|------------|-------|--------|---------|----------|-----------|
| name_match | 6 266 | 103    | 1.6     | 1 163    | 18.6      |
| imported   | 267   | 51     | 19.1    | 128      | 47.9      |
| **local**  | 1 214 | 0      | 0.0     | **3**    | **0.2**   |
| external   | 414   | 0      | 0.0     | 0        | 0.0       |

`local` reste à 0.2% **alors que les défs de locals/params SONT bien
émises** par l'extracteur (`deep=true` activé, vérifié dans le smoke
layer 4 : `defs=4 links=2` pour `function bar(x: number) {…} qux()`).
Le strip ne change rien parce que le mismatch n'est pas dans le name —
il est dans le path complet.

Reproduit par requête directe sur la DB ingest :

```sql
-- target ref read sur la variable `m` lue dans main()
TARGET     : esac+moniker://esac/.../path:dogfood_full_import/function:m()
TARGET_STRIP: esac+moniker://esac/.../path:dogfood_full_import/function:m

-- def correspondante (local de main(), bien émise grâce à deep=true)
DEF        : esac+moniker://esac/.../path:dogfood_full_import/function:main()/local:m
```

`bind_match(target, def)` retourne `false` :

```sql
SELECT
  'esac+moniker://esac/.../path:dogfood_full_import/function:m'::moniker
  ?=
  'esac+moniker://esac/.../path:dogfood_full_import/function:main()/local:m'::moniker;
-- → false
```

Confirmé : `bind_match` exige égalité de **tous** les segments parents
(modulo last-kind), il ne fait pas de descente.

## Code source incriminé

`src/lang/ts/refs.rs:299-315` — `emit_read_at` reçoit `scope` mais ne
le propage pas à `read_target` :

```rust
pub(super) fn emit_read_at(
    &self,
    node: Node<'_>,
    scope: &Moniker,                       // ← reçu
    graph: &mut CodeGraph,
) {
    let name = self.text_of(node);
    if name.is_empty() { return; }
    let target = self.read_target(name);   // ← scope IGNORÉ ici
    let attrs = RefAttrs {
        confidence: self.name_confidence(name.as_bytes()),
        ..RefAttrs::default()
    };
    let _ = graph.add_ref_attrs(scope, target, kinds::READS, ...);
}
```

`src/lang/ts/refs.rs:412-414` — `read_target` construit depuis
`self.module`, jamais depuis le scope :

```rust
fn read_target(&self, name: &str) -> Moniker {
    extend_callable_typed(&self.module, kinds::FUNCTION, name.as_bytes(), &[] as &[&[u8]])
    //                    ^^^^^^^^^^^^ parent = module brut
}
```

Idem `src/lang/ts/refs.rs:396-402` (`calls_target` et
`method_call_target` utilisent tous `self.module` comme parent).

Idem `src/lang/ts/refs.rs:404-414` (`instantiates_target`,
`heritage_target`, `read_target`).

À l'inverse, **`emit_param_leaf` et `handle_for_in` côté défs**
(`walker.rs:455-462` et `:496-512`) construisent CORRECTEMENT depuis
`callable` / `scope` — c'est cette asymétrie qui crée le mismatch.

## Test qui aurait catché le bug

Variante du test #1 du bug 001 — closure intra-module **avec navigation
dans un nested scope** :

```rust
#[test]
fn read_inside_function_target_must_bind_match_local_def() {
    let g = extract("util.ts",
        "function main() { let m = 1; return m + 1; }",
        &make_anchor(), /*deep=*/true);
    let defs: Vec<_> = g.defs().map(|d| d.moniker.clone()).collect();
    for r in g.refs().filter(|r| r.confidence == b"local") {
        assert!(
            defs.iter().any(|d| bind_match(d, &r.target)),
            "DANGLING local ref: target={} (no path overlap with any def)\n  Defs: {:?}",
            r.target, defs
        );
    }
}
```

Avec le code actuel : ÉCHOUE. Le local `m` est défini sous
`util/function:main()/local:m`, mais le ref `m` est émis vers
`util/function:m()`. Pas de bind_match possible — paths disjoints.

Le test du bug 001 (sans `deep`) n'aurait pas catché celui-ci puisque
`deep=false` empêche les défs de locals d'exister du tout. Il faut
explicitement `deep=true` dans le test.

## Pistes de fix (à arbitrer)

Trois pistes plausibles ; aucune triviale :

1. **`read_target` / `calls_target` prennent le scope** comme parent au
   lieu de `self.module`. Mais ça fait pointer la ref vers
   `function:main()/function:m()` qui n'existe pas non plus comme def
   (le def est sous `local:m`, pas `function:m()`). Donc il faut aussi
   un mécanisme de **search ancestor** au moment de l'émission ou du
   match : remonter l'arbre des scopes jusqu'à trouver un identifiant
   ce nom.

2. **Faire l'extracteur résoudre le scope au moment de l'émission**.
   Maintenir une scope-table (déjà partiellement présente via
   `Walker::local_scope`) qui mappe `name → moniker complet du local
   défini`. Quand `emit_read_at` voit un identifiant local, il
   récupère le moniker complet depuis cette table. C'est le bon design
   mais demande de plumber l'info à travers tout le walker.

3. **Côté caller (ESAC)** : faire de la résolution avec une stratégie
   "ancestor sweep". Pour un target avec `confidence='local'`, ne pas
   matcher exactement le path — chercher tout def `d` tel que
   `d.moniker_stripped` finit par le `last_segment_name(target)` ET
   est sous `module_root(target)`. Possible mais explose la
   sémantique de `?=` et coûte cher en queries.

## Lien avec bug 001

Sans le fix bug 001 (asymétrie name) le strip ne peut rien faire
contre cette cause #3. Avec le fix bug 001, il reste **0.2% de local
résolus** — preuve que ce bug est distinct et persiste.

Le test recommandé en queue de bug 001
(`assert_local_refs_closed`) catche les deux bugs en même temps —
c'est un point en faveur de l'invariant comme test universel.
