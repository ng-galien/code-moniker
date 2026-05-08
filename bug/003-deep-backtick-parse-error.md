# 003 — `moniker parse error: unterminated backtick-quoted name` avec `deep=true`

**Reporté** : 2026-05-08, en activant `deep=true` côté caller (ESAC v2).
**État** : ouvert.
**Composant** : `src/core/moniker/...` (parser texte) ou
`src/lang/ts/walker.rs` (emitter qui produit du moniker text avec
backticks non échappés).

## Symptôme

`extract_typescript(uri, source, anchor, deep=true)` plante sur 68 / 178
fichiers TS du repo `esac` avec :

```
ERROR: moniker parse error: unterminated backtick-quoted name at byte N
```

`deep=false` ne déclenche jamais cette erreur (mesuré avant fix bug 001 :
175 / 177 fichiers extraient sans erreur, les 2 manquants sont des
binary skips).

## Hypothèse de cause

Le format texte des monikers utilise des backticks pour quoter les
names contenant des caractères spéciaux. Vu dans la couche v2 ESAC
au commit `d25dc25`, def #11 d'un module :

```
.../function:`extractCommitsLocally(string,string,string,string | null)`
```

Le name `extractCommitsLocally(string,string,string,string | null)` est
backtick-quoted parce qu'il contient des espaces et `|`. C'est le name
typé d'une fonction.

En `deep=true`, l'extracteur émet aussi les **params** et **locals** comme
défs. Si un nom de param ou de local contient un backtick littéral
(par ex. dans une template literal en TS, ou un identifier exotique),
le backtick côté quoting du moniker text s'imbrique avec le backtick
dans le name → unbalanced quoting → parse error en re-cast moniker.

Vérification possible : grepper les fichiers échouants pour des
backticks proches du byte offset reporté. Les 5 premiers échecs ont des
offsets très bas (61-112), donc probablement dans un import statement
ou une top-level const avec template literal.

## Échantillon

```
[extract] esac://esac/scripts/smoke-bridge.ts: parse error at byte 65
[extract] esac://esac/scripts/smoke-sync.ts:    parse error at byte 63
[extract] esac://esac/src/api/auth.ts:           parse error at byte 62
[extract] esac://esac/src/api/pool.ts:           parse error at byte 61
[extract] esac://esac/src/cli/commands/install.ts: parse error at byte 112
```

Ces fichiers extrayaient bien en `deep=false`. La régression est donc
**dans le chemin emit-def des params/locals** quand `deep=true`.

## Test qui catcherait le bug

Test Rust unitaire avec un identifier "exotique" — backtick dans une
template literal apparaissant comme var name :

```rust
#[test]
fn extract_with_template_literal_in_local_does_not_break_moniker_text() {
    // const greet = `hello`; — top-level const using template literal
    let g = extract("util.ts",
        "const greet = `hello`; export function f() { let x = greet; }",
        &make_anchor(), /*deep=*/true);
    // L'extraction doit aboutir sans erreur, et tous les monikers
    // émis doivent re-parser correctement.
    for d in g.defs() {
        let s = d.moniker.to_string();
        let _: Moniker = s.parse().expect(&format!("def moniker '{}' must roundtrip", s));
    }
    for r in g.refs() {
        let s = r.target.to_string();
        let _: Moniker = s.parse().expect(&format!("ref target '{}' must roundtrip", s));
    }
}
```

L'invariant général : **tout moniker émis doit roundtrip via
`moniker_out` / `moniker_in`** (équivalent SQL : `m::text::moniker = m`).
À ajouter comme assertion finale dans tous les tests d'extraction.

## Reproduction côté ESAC

Le smoke ESAC layer 04_symbol passe (snippet TS minimal sans backtick).
La régression apparaît uniquement sur du code de prod. Le script
`db/v2/scripts/dogfood_full_import.mjs` côté ESAC reproduit en boucle :
chaque fichier listé en "first 5 failures" du log produit le même
SQLSTATE.

## Pistes de fix

1. **Échapper les backticks** dans le name avant de les quoter.
   Dans `moniker_out` ou dans le constructeur de segment, si un `\``
   apparaît dans le name, l'échapper en `\\\``. Symétrique côté parser.

2. **Trouver une autre stratégie de quoting** pour les names avec
   backticks — guillemets doubles, brackets, encoding hex, ...

3. **Refuser au moment de l'émission** un name contenant un backtick
   et fallback sur un placeholder (`__nontextable_<hash>`). Plus
   défensif mais perd l'info.

## Sévérité

Bloquant pour `deep=true` à l'échelle du repo : 38% des fichiers TS
d'esac touchés. Tant que ce bug existe, le bug 001 cause #2 (locals
absents) reste partiellement masqué — on ne peut pas activer `deep`
sans perdre 38% de la couverture.

Workaround côté caller : aucun. C'est un bug de parsing du moniker text,
inattaquable hors extension.
