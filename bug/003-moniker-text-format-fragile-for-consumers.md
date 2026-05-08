# 003 — le format texte du moniker piège les consumers qui le manipulent

**Reporté** : 2026-05-08, observé pendant le dogfood ESAC layer 4.
**État** : ouvert. Pas un bug de parser ni d'extracteur ; concern de
surface API.

## Symptôme observable

Pendant le dogfood-import du repo ESAC sur la couche v2, 68/178
fichiers TS plantent avec :

```
moniker parse error: unterminated backtick-quoted name at byte N
```

Investigation par bisect : `extract_typescript(uri, source, anchor,
deep=true)` est innocent — sur les contenus intégraux des fichiers
incriminés, l'extraction passe sans erreur. La cassure intervient en
aval, dans une fonction côté caller (ESAC) qui fait
`regexp_replace(m::text, …)::moniker` pour produire une forme
"strippée" du moniker (workaround temporaire en attendant le fix
bug 001 cause #1, branche TS dans `last_segment_match`).

La regex consumer-side ne savait pas que les names contenant des
caractères spéciaux (espaces, `|`, …) sont entourés de backticks dans
la forme texte. Stripper le `(...)` final retirait aussi le backtick
fermant, laissant le quoting non balancé — le cast `::moniker` rejette.

**C'est un bug consumer-side, pas extension-side.** Le doc reste ici
parce que la cause racine est une **fragilité d'API** : la forme texte
du moniker n'est pas conçue pour être manipulée par regex côté
consumer, mais l'extension n'expose pas de primitive sûre pour les
opérations courantes (drop signature, drop kind, parent path, …).

## Pourquoi ça mérite un fichier ici

L'observation directe : dès que la couche symbolique est consommée
sérieusement par un caller (cas d'usage : SCIP-style refinement
côté ESAC en attendant la branche TS dans `last_segment_match`), le
caller a besoin d'opérations sur le name du dernier segment. Trois
options aujourd'hui :

1. **Manipuler `moniker_out(m)` par regex** — fragile, demande au
   caller de connaître les règles de quoting (backticks, espaces,
   futurs ajouts). Cassures silencieuses au passage du round-trip.

2. **Décomposer via `graph_defs(graph)` / segments accesseurs** —
   l'extension expose `graph_defs` qui retourne `(moniker, kind,
   visibility, signature, binding, start_byte, end_byte)`. Mais
   `signature` est juste pour les défs ; côté ref-target, il n'y a pas
   de séparation name/signature. Le caller qui veut comparer un name
   nu de def à un name nu de ref doit recoder le strip lui-même.

3. **Demander à l'extension** une primitive type
   `bare_callable_name(moniker) -> moniker` (analogue de la fonction
   `bare_callable_name` privée déjà dans `src/core/moniker/query.rs`
   pour la branche SQL de `last_segment_match`) **publique**. Le
   workaround consumer-side disparaît, le quoting reste opaque.

L'option 3 est cohérente avec la philosophie "bind_match est l'API
que les consumers utilisent". Aujourd'hui `bind_match` fait le strip
SQL en interne (lignes 78-79 de `query.rs`), mais ne l'expose pas
comme fonction réutilisable. Si un consumer veut le faire en amont
(par ex. pour indexer une colonne dérivée stripped), il duplique la
logique.

## Tests qui auraient catché

Au niveau extension :

```rust
#[test]
fn moniker_text_roundtrip_for_backtick_quoted_names() {
    // Tous les formats émis par les extractors doivent roundtrip.
    let cases = [
        b"function:foo(int,String)".as_ref(),
        b"function:`foo with spaces`",
        b"function:`f((x: number) => string)`",      // template-like type
        b"function:`f(string | null)`",              // pipe in type
    ];
    for name in cases {
        let m = mk(b"app", &[(b"path", b"x"), (b"function", name)]);
        let s = m.to_string();
        let parsed: Moniker = s.parse().unwrap_or_else(|e| {
            panic!("roundtrip failed on {}: {}", s, e)
        });
        assert_eq!(parsed, m, "roundtrip mismatch for {}", s);
    }
}
```

Au niveau consumer (côté ESAC, déjà ajouté) :

Le test consumer-side n'aurait pas catché le bug parce que mon
workaround utilise une regex hand-rolled — c'est le consumer qui doit
recoder le strip qui est risqué. Le test côté extension exposerait le
même risque pour tout futur consumer.

## Pistes de fix

Trois pistes par ordre de pouvoir, **toutes côté extension** :

1. **Exposer `bare_callable_name(moniker) -> moniker`** comme
   fonction SQL publique (et Rust pub). Le caller appelle ça au lieu
   de regex-ploter. Travail : promouvoir la fonction privée
   `query.rs:85-90` en publique + l'enrober côté pgrx + un test
   roundtrip.

2. **Améliorer le message d'erreur du parser** — actuellement
   "unterminated backtick-quoted name at byte N". Préciser la position
   dans la string source du backtick non fermé, et idéalement un hint
   "consumer regex on moniker text? Use bare_callable_name() instead".

3. **Documenter explicitement** dans `MONIKER_URI.md` ou `SPEC.md`
   que la forme texte est un format de transport, pas une API ;
   toute manipulation de segments doit passer par les accesseurs
   typés. Pas de fix code, mais empêche le prochain consumer de
   tomber dans le piège.

## Lien avec bug 001

Cette fragilité d'API ne se manifeste que parce que le bug 001 cause
#1 (asymétrie name def/ref pour TS) force les consumers à coder leur
propre strip. Si la branche TS est ajoutée à `last_segment_match`
(comme c'est déjà fait pour SQL), le consumer n'a plus besoin de
manipuler la forme texte du moniker du tout. Le risque API
disparaît avec bug 001 fix.

Ordre suggéré : **fixer bug 001 d'abord** (simple, branche TS dans
`last_segment_match`), ensuite bug 003 devient académique. La
piste 1 (`bare_callable_name` public) reste utile en backup pour les
consumers qui voudraient projeter en colonne dénormalisée.

## Observation factuelle

Sur le repo ESAC v2 (commit `7484be6`), avec le workaround consumer
backtick-aware (regex consumer corrigée), 176/178 fichiers
extraient. Sans ce fix consumer-side, 68/178 plantaient. Le risque
existe pour tout autre consumer qui voudrait faire la même
opération.
