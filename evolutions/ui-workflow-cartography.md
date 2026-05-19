# Cartographie des workflows UI

Objectif : documenter les contrats concrets de l'UI actuelle avant de
refactorer l'architecture. Cette cartographie ne propose pas encore une
réécriture ; elle isole les chemins où les décisions sont aujourd'hui
dispersées entre `AppState::reduce`, les `Effect`, les handlers `App`, le
`WorkspaceStore` et la navigation.

## Lecture rapide

Les workflows à plus forte friction sont :

- recherche header ;
- navigation arbre ;
- usage lens et mode changements ;
- focus et navigation du panel ;
- rafraîchissement workspace.

Pour chacun, la question utile est : à partir d'un message utilisateur ou d'un
événement shell, quelles données sont lues, quelles mutations sont produites,
quels effets runtime sont nécessaires, et quel contrat devrait être testable ?

## Workflow 1 : recherche header

### Entrées

- `Msg::ToggleHeaderSearch`
- `Msg::HeaderSearchInput`
- `Msg::HeaderSearchApply`
- `Msg::HeaderSearchReset`
- `Msg::HeaderSearchSelectNext`
- `Msg::HeaderSearchSelectPrevious`
- `Msg::HeaderSearchToggleSelection`
- `AppAction::HeaderSearchDebounced`

### Chemin actuel

Le reducer modifie parfois directement `ShellSlice.header_search`, mais délègue
les décisions principales via `Effect` :

- `HeaderSearchInput` édite le draft, écrit le `status`, puis émet
  `Effect::DebounceHeaderSearch`.
- `HeaderSearchApply` émet `Effect::ApplyHeaderSearch`.
- `HeaderSearchReset` reset l'état de recherche puis émet
  `Effect::ApplyHeaderSearch`.
- Les sélecteurs lang/kind passent par `Effect::CycleHeaderSearchSelector` et
  `Effect::ToggleHeaderSearchSelection`.

`App::apply_header_search` relit ensuite le header, interroge le
`WorkspaceStore`, applique le filtre, reconstruit les résultats visibles,
sélectionne le premier match, synchronise la vue contextuelle et écrit le
status.

### Données lues

- `AppState.shell.mode`
- `AppState.shell.header_search`
- `AppState.shell.active_filter`
- options disponibles du header search
- `WorkspaceStore::search_symbols_filtered`
- `WorkspaceStore::all_navigable_defs`
- `WorkspaceStore::changed_defs`
- `WorkspaceStore::stats`

### Mutations produites

- mode normal/header search ;
- draft de recherche ;
- filtres actifs ;
- options/cursors lang et kind ;
- scope de navigation ;
- sélection primaire ;
- vue contextuelle ;
- focus éventuel ;
- status.

### Effets runtime réels

- debounce asynchrone ;
- aucun accès terminal direct ;
- aucune I/O hors lecture du workspace déjà chargé.

### Friction

`ApplyHeaderSearch` n'est pas un effet runtime : c'est une suite de décisions
UI synchrones. Le contrat "appliquer une recherche" est donc réparti entre le
reducer, `runtime.rs`, `header_search.rs`, `navigation.rs` et le store.

### Contrats à tester

- Une recherche vide efface le filtre et restaure le scope attendu.
- Une recherche avec résultats met `ActiveFilter::HeaderSearch`, reconstruit la
  navigation en scope filtré, sélectionne le premier match et met la vue
  contextuelle correcte.
- Un debounce périmé ne modifie rien.
- Les filtres lang/kind changent les résultats et les labels sans perdre le
  focus attendu.

## Workflow 2 : navigation arbre

### Entrées

- `Msg::MoveDown`
- `Msg::MoveUp`
- `Msg::Home`
- `Msg::End`
- `Msg::ToggleNode`
- `Msg::OpenNode`
- `Msg::CloseNode`
- actions indirectes après recherche, change mode, usage lens ou reload.

### Chemin actuel

`AppState::reduce_ui_msg` choisit souvent une `NavigationAction` et l'encapsule
dans `Effect::Navigation`. `App::apply_navigation` appelle ensuite
`dispatch_navigation`, puis synchronise la vue contextuelle si la navigation a
changé.

`dispatch_navigation` possède une logique supplémentaire : il compare la
sélection avant/après, applique les effets émis par `NavigationState`, puis
reset la navigation du panel si la sélection a changé.

### Données lues

- focus courant (`Navigator`, `UsageLens`, `Panel`) ;
- navigation primaire et usage lens ;
- sélection courante ;
- filtre actif ;
- usage lens ouvert ou non ;
- workspace indirectement lors des reconstructions de modèles.

### Mutations produites

- cursor de navigation ;
- expansion/collapse de noeuds ;
- scope de navigation ;
- sélection primaire ou usage lens ;
- reset du panel navigation ;
- vue contextuelle ;
- status open/close.

### Effets runtime réels

Aucun. C'est de la transition UI pure.

### Friction

Il existe deux chemins de réduction : `AppAction -> AppState` et
`NavigationAction -> NavigationState`. Le second est déclenché par un `Effect`
du premier, puis enrichi par `App` avec reset panel, status et sync view.

### Contrats à tester

- Déplacer la sélection reset le panel navigation.
- Ouvrir/fermer un noeud met le status attendu.
- `Esc` ferme le noeud sélectionné, ou remonte le focus, ou clear le scope
  selon le contexte.
- Une action navigation qui change la sélection synchronise la vue
  contextuelle.

## Workflow 3 : usage lens et mode changements

### Entrées

- `Msg::FocusUsages`
- `Msg::ToggleChangeMode`
- `Msg::CloseNode` quand le focus est dans `UsageLens`
- navigation primaire ou usage lens.

### Chemin actuel

`Msg::FocusUsages` émet `Effect::FocusUsages`, puis `App::focus_usages_of_selected`
décide selon le mode :

- si la vue est en mode changements, toggle le panel diff/usages ;
- si une usage lens est déjà ouverte, la ferme ;
- sinon lit la sélection primaire, construit une `UsageFocus` depuis le
  workspace, met à jour le shell, reconstruit la navigation usage lens,
  synchronise la vue et écrit le status.

`Msg::ToggleChangeMode` suit le même style : `Effect::ToggleChangeMode`, puis
`App::toggle_change_mode`.

### Données lues

- mode de visualisation ;
- filtre actif ;
- sélection primaire ;
- usage lens courante ;
- change detail sélectionné ;
- `WorkspaceStore::usage_focus` ;
- `WorkspaceStore::change_overview` ;
- `WorkspaceStore::changed_defs`.

### Mutations produites

- `usage_lens` dans le shell ;
- focus region ;
- scope navigation usage lens ;
- mode changements ;
- filtre actif ;
- sélection du premier changement ;
- panel `Diff` ou `Usages` ;
- vue contextuelle ;
- status.

### Effets runtime réels

Aucun.

### Friction

Le même raccourci utilisateur (`u`) peut ouvrir une usage lens, fermer une usage
lens ou basculer le panel de changement. Cette logique est correcte mais
implicite dans `App`, donc difficile à valider comme contrat d'état.

### Contrats à tester

- `u` sans sélection affiche un status et ne modifie pas le scope.
- `u` avec sélection ouvre une usage lens, peuple la navigation secondaire et
  met le focus/vue attendus.
- `u` quand une usage lens est ouverte la ferme et restaure le contexte.
- En mode changements, `u` alterne entre diff et usages sans quitter le mode.
- `d` entre en mode changements, reconstruit le scope change et sélectionne le
  premier changement.

## Workflow 4 : focus et navigation du panel

### Entrées

- `Msg::ToggleFocusRegion`
- `Msg::PanelScrollDown`
- `Msg::PanelScrollUp`
- `Msg::MoveDown` / `MoveUp` quand le focus est `Panel`
- `Msg::Home` / `End` quand le focus est `Panel`
- `Msg::CopyPanelSnapshot`

### Chemin actuel

Le reducer délègue presque tout via `Effect` :

- `ToggleFocusRegion`
- `PanelMove`
- `PanelHome`
- `PanelEnd`
- `CopyPanelSnapshot`

Les handlers `App` reconstruisent le panel actif (`explorer::active_panel`),
calculent sa longueur navigable et son composant, puis mutent
`PanelNavigationState` ou lancent une copie clipboard.

### Données lues

- focus region ;
- usage lens ouverte ;
- vue active ;
- filtre/scope courant ;
- panel actif construit depuis `App` ;
- `PanelNavigationState`.

### Mutations produites

- focus region ;
- sélection interne du panel ;
- scroll du panel ;
- status.

### Effets runtime réels

- copie clipboard asynchrone pour `CopyPanelSnapshot`.

### Friction

La navigation du panel dépend d'une VM/panel construit depuis `App`, pas d'un
modèle stable de données. Une action d'état UI doit donc appeler le rendu
intermédiaire pour savoir combien d'items sont navigables.

### Contrats à tester

- Passer le focus au panel initialise une sélection si le panel contient des
  items.
- Déplacer dans le panel borne la sélection entre `0` et `len - 1`.
- Si le panel n'a pas d'item navigable, les touches déplacent le scroll.
- Changer de composant panel reset la sélection/scroll de manière prévisible.
- Copier un panel produit une commande clipboard avec le bon texte et met le
  status de copie.

## Workflow 5 : rafraîchissement workspace

### Entrées

- `AppAction::Store(StoreEvent::FullIndex)`
- `AppAction::Store(StoreEvent::GitOverlay)`
- `AppAction::TaskCompleted`
- startup load.

### Chemin actuel

Le reducer invalide les époques de travail et marque l'état changé. `App`
interprète ensuite l'événement store, lance une tâche async si possible, ou
applique le refresh en synchrone.

Après un reload ou un refresh git, `App` :

- met à jour le `WorkspaceStore` ;
- pose `watch_roots_update` pour la boucle terminale ;
- refresh les options de recherche ;
- reconstruit les modèles de navigation ;
- refresh le filtre actif ;
- refresh l'usage lens si nécessaire ;
- refresh les résultats ;
- sélectionne le premier changement dans certains cas ;
- synchronise la vue contextuelle ;
- écrit le status.

### Données lues

- options de session ;
- snapshot workspace ;
- filtre actif ;
- usage lens courante ;
- navigation actuelle.

### Mutations produites

- workspace snapshot ;
- watch roots pending ;
- header search options ;
- active filter recalculé ;
- usage lens recalculée ;
- navigation models ;
- visible defs ;
- sélection ;
- vue contextuelle ;
- status ;
- work epochs.

### Effets runtime réels

- tâche async reload store ;
- tâche async refresh git overlay ;
- mise à jour des watchers terminal.

### Friction

Ce workflow mélange légitimement I/O, workspace et UI. La friction vient surtout
du fait que le résultat UI post-refresh est une orchestration impérative, sans
objet de décision intermédiaire testable.

### Contrats à tester

- Un reload conserve/recalcule le filtre header search.
- Un reload recalcule l'usage lens à partir du target précédent.
- En mode change avec scope vide, le premier changement est sélectionné après
  refresh.
- Un résultat de tâche périmé est ignoré et ne remplace pas l'état courant.
- Les watchers sont remplacés uniquement après un store reload/catalog valide.

## Découpes proposées pour refactor incrémental

### Read model UI

Créer une interface de lecture minimale pour les décisions UI, séparée du
runtime :

```rust
trait UiWorkspaceRead {
	fn stats_def_count(&self) -> usize;
	fn all_navigable_defs(&self) -> Vec<DefLocation>;
	fn changed_defs(&self) -> Vec<DefLocation>;
	fn search_header(
		&self,
		text: &str,
		langs: &[Lang],
		kinds: &[HeaderKindFilter],
	) -> HeaderSearchResults;
	fn usage_focus(&self, loc: DefLocation) -> UsageFocus;
	fn change_overview(&self) -> ChangeOverview;
}
```

Le nom exact peut changer. Le point important est que les décisions UI ne
prennent pas `&App`.

### Décision explicite

Introduire une sortie de décision simple, par exemple :

```rust
struct UiDecision {
	shell: Vec<ShellAction>,
	navigation: Vec<NavigationAction>,
	runtime: Vec<RuntimeCommand>,
}
```

Cela permet de tester "voici les actions produites" avant de tester leur
application réelle.

### Premier vertical slice conseillé

Commencer par la recherche header :

- surface utilisateur importante ;
- friction forte ;
- peu d'I/O réelle ;
- dépendances workspace simples ;
- contrats faciles à tester.

Ne pas commencer par le reload workspace : il mélange trop de responsabilités
et donnera une abstraction trop large trop tôt.

## Questions ouvertes

- Les `status` doivent-ils rester des mutations directes, ou devenir des
  résultats de décision nommés ?
- `NavigationAction` doit-elle rester un reducer secondaire ou devenir une
  branche de `AppAction` ?
- Le panel actif doit-il exposer un read model navigable indépendant de la VM
  de rendu ?
- Les compteurs `generation` doivent-ils piloter le redraw, ou être supprimés
  hors époques de travail async ?

## Analyse topologique du code actuel

Cette section relit les assertions précédentes depuis le code. Elle vise la
localisation des responsabilités : où le comportement est décidé, où l'état est
muté, où le runtime intervient, et où l'organisation rend le flux plus difficile
à maîtriser.

### Topologie globale

Le chemin nominal d'une action utilisateur est :

```text
terminal event
  -> key_to_msg
  -> App::update(AppAction::Ui)
  -> AppStore::dispatch
  -> AppState::reduce_ui_msg
  -> Transition.effects
  -> App::apply_effect
  -> handlers App
  -> dispatch_shell / dispatch_navigation / queue_task
  -> redraw via ExplorerVm::from_app
```

Localisation :

- la boucle terminale reçoit les événements dans
  `crates/cli/src/ui/shell/terminal.rs` (`handle_app_events`) ;
- `App::update` orchestre dispatch, effets et événements spéciaux dans
  `crates/cli/src/ui/app/runtime.rs:119` ;
- `AppStore` combine `ReducerStore<AppState>` et `WorkspaceStore` dans
  `crates/cli/src/ui/app/store.rs:9` ;
- `AppState::reduce_ui_msg` décide une partie du comportement clavier dans
  `crates/cli/src/ui/app/state.rs:399` ;
- `App::apply_effect` redirige les effets vers des méthodes `App` dans
  `crates/cli/src/ui/app/runtime.rs:169` ;
- la VM est reconstruite depuis `App` dans
  `crates/cli/src/ui/explorer/vm.rs:106`.

Friction topologique : le nommage suggère une séparation claire
`state/reducer/effect/runtime`, mais plusieurs effets sont des transitions UI
synchrones. Le comportement réel d'une action se trouve donc souvent dans
trois zones : reducer, interpréteur d'effet, handler `App`.

### Assertion : le reducer délègue des décisions UI synchrones

Localisation :

- recherche : `HeaderSearchApply` et `HeaderSearchReset` émettent
  `Effect::ApplyHeaderSearch` dans `state.rs:435` et `state.rs:444` ;
- usage lens et change mode : `FocusUsages` et `ToggleChangeMode` émettent des
  effets dans `state.rs:471` et `state.rs:472` ;
- panel : `CopyPanelSnapshot`, `PanelMove`, `PanelHome`, `PanelEnd` passent par
  des effets dans `state.rs:473`, `state.rs:552`, `state.rs:580` ;
- navigation : `MoveDown`, `MoveUp`, `Home`, `End` construisent des
  `NavigationAction` enveloppées dans `Effect::Navigation` dans
  `state.rs:524` et `state.rs:556`.

Interprétation :

Ces effets ne sont pas tous du runtime. `DebounceHeaderSearch`, `RunCheck`,
`CopyPanelSnapshot` et `Quit` ont une dimension runtime. En revanche,
`ApplyHeaderSearch`, `FocusUsages`, `ToggleChangeMode`, `PanelMove`,
`Navigation`, `OpenSelectedNode` et `CloseNodeOrClearScope` poursuivent surtout
une transition UI.

Friction organisationnelle :

Le fichier `state.rs` contient les types d'état et une partie des décisions,
mais les branches les plus importantes sortent du fichier avant de produire
l'état final. Pour relire un workflow, il faut sauter de `state.rs` vers
`runtime.rs`, puis vers un fichier `app/*.rs` spécialisé.

Approche lean :

Ne pas créer une couche abstraite générale immédiatement. Commencer par
renommer ou séparer les sorties :

- `RuntimeCommand` pour debounce, tâches, clipboard, quit ;
- appels directs ou petites fonctions de décision pour les transitions UI
  synchrones.

### Assertion : recherche header est répartie sur trop de lieux

Localisation du flux :

- entrée clavier et état draft : `state.rs:422` édite le texte, écrit le
  status et émet le debounce ;
- application : `state.rs:444` émet `Effect::ApplyHeaderSearch` ;
- interprétation : `runtime.rs:176` appelle `App::apply_header_search` ;
- décision principale : `header_search.rs:176` relit le header, calcule les
  résultats, applique le filtre, refresh la navigation, sélectionne le premier
  match, synchronise la vue et écrit le status ;
- calcul workspace : `header_search.rs:221` appelle
  `explorer_header_search_results(self.store(), ...)` ;
- refresh navigation : `navigation.rs:85` calcule les defs visibles et envoie
  `NavigationAction::SetScope` ;
- choix des defs visibles : `navigation.rs:96` lit soit les matches du filtre,
  soit `changed_defs`, soit `all_navigable_defs` ;
- clear filtre : `header_search.rs:358` clear le shell, clear usage lens,
  refresh les résultats, sync la vue et écrit le status.

Données et mutations vérifiées :

- `ShellAction::ApplyHeaderSearch` est réduit dans `state.rs:260` ;
- `ShellAction::SetHeaderSearchFilters` est réduit dans `state.rs:264` ;
- `ShellAction::ClearFilter` est réduit dans `state.rs:286` ;
- les options du sélecteur sont recalculées depuis le workspace dans
  `header_search.rs:342`.

Friction organisationnelle :

`header_search.rs` contient à la fois :

- helpers purs de label/sélecteur ;
- handlers `App` ;
- lecture workspace ;
- orchestration navigation ;
- écriture status.

Ce n'est pas une erreur fatale, mais le contrat "appliquer une recherche" n'est
pas localisé. Il est plus compliqué que nécessaire de tester seulement la règle
produite par Enter.

Approche lean :

Extraire une fonction locale au module, sans nouvelle architecture globale :

```rust
fn decide_apply_header_search(
	state: &AppState,
	workspace: &impl IndexStore,
	generation: Option<u64>,
	return_focus: bool,
) -> HeaderSearchDecision
```

`HeaderSearchDecision` peut rester simple : actions shell, actions navigation,
sélection optionnelle, status. Si ce type devient trop gros, c'est le signal
qu'il faut redécouper, pas l'inverse.

### Assertion : navigation a un second reducer et des compléments dans `App`

Localisation :

- `AppStore::dispatch_navigation` appelle `NavigationState::reduce` dans
  `store.rs:92` et incrémente manuellement les générations dans
  `store.rs:101` ;
- `App::dispatch_navigation` compare la sélection avant/après et reset le panel
  dans `navigation.rs:72` ;
- `App::apply_navigation` synchronise la vue contextuelle après changement dans
  `navigation.rs:238` ;
- les statuts open/close sont produits dans `toggle_selected_nav`,
  `open_selected_nav` et `close_selected_nav` (`navigation.rs:191`,
  `navigation.rs:203`, `navigation.rs:213`).

Friction organisationnelle :

La responsabilité "navigation" est séparée en trois niveaux :

- `ui/store/navigation*` pour l'état arbre et le reducer ;
- `ui/app/state.rs` pour choisir certaines actions selon le focus ;
- `ui/app/navigation.rs` pour compléter avec panel reset, vue contextuelle,
  status et accès workspace.

Cette séparation est compréhensible, mais le contrat utilisateur n'est pas dans
un seul endroit. Par exemple "une sélection change donc le panel reset" est
dans `App::dispatch_navigation`, pas dans le reducer navigation ni dans le
reducer app.

Approche lean :

Conserver le reducer navigation tant qu'il reste utile. Déplacer en priorité les
compléments systématiques près du dispatch unique :

- changement de sélection -> reset panel ;
- changement de sélection -> sync contextual view ;
- notice open/close -> status.

Un petit type `NavigationOutcome` serait plus simple qu'une fusion immédiate de
tous les reducers.

### Assertion : usage lens et change mode mélangent plusieurs règles métier UI

Localisation :

- `Msg::FocusUsages` délègue à `Effect::FocusUsages` dans `state.rs:471` ;
- `runtime.rs:184` appelle `focus_usages_of_selected` ;
- `focus_usages_of_selected` multiplexe trois comportements dans
  `usage_lens.rs:26` : mode change, fermeture lens existante, ouverture depuis
  sélection ;
- ouverture usage lens : `usage_lens.rs:7` lit `WorkspaceStore::usage_focus`,
  met `ShellAction::SetUsageLens`, envoie `NavigationAction::SetUsageLens`,
  sync la vue et écrit le status ;
- fermeture usage lens : `usage_lens.rs:42` clear le shell, clear la navigation
  lens, sync la vue et écrit le status ;
- mode changements : `usage_lens.rs:53` entre en change mode, refresh les
  résultats, sélectionne le premier changement, sync la vue et écrit le status ;
- toggle diff/usages en change mode : `usage_lens.rs:69`.

Friction organisationnelle :

Le fichier s'appelle `usage_lens.rs`, mais il contient aussi `toggle_change_mode`
et `toggle_change_usages`. Ce n'est pas absurde fonctionnellement, car le
raccourci `u` interagit avec le mode changements, mais l'organisation cache que
deux parcours utilisateur différents cohabitent.

Approche lean :

Découper par parcours plutôt que par type technique :

- garder `usage_lens.rs` pour ouvrir/fermer/recharger une lens ;
- déplacer `toggle_change_mode` et `toggle_change_usages` vers un module
  `change_mode.rs` ou `change_panel.rs` ;
- garder les fonctions courtes et explicites plutôt que créer un framework de
  workflow.

### Assertion : panel focus dépend de la VM de rendu

Localisation :

- `Msg::ToggleFocusRegion` sort via effet dans `state.rs:404` ;
- `runtime.rs:189` appelle `toggle_focus_region` ;
- `toggle_focus_region` choisit la prochaine région et initialise le panel dans
  `panel_focus.rs:41` ;
- `ensure_active_panel_selection`, `move_panel_selection` et
  `move_panel_to_edge` reconstruisent le panel actif via
  `explorer::active_panel(self)` dans `panel_focus.rs:65`, `panel_focus.rs:91`
  et `panel_focus.rs:118` ;
- `explorer::active_panel` délègue à `panel_content::active_panel` dans
  `explorer/mod.rs:13` ;
- `panel_content::active_panel` choisit le panel selon `app.view()` dans
  `panel_content.rs:9`.

Friction organisationnelle :

Une décision d'état UI ("où est la sélection dans le panel ?") dépend d'un objet
de présentation (`PanelVm`) construit depuis `App`. C'est le point le plus net
où rendu/view-model et contrôle se touchent dans le mauvais sens.

Approche lean :

Ne pas extraire tout le rendu. Ajouter seulement une petite fonction ou un petit
read model :

```rust
fn active_panel_nav(app: &App) -> PanelNavModel {
	component: ComponentId,
	navigation_len: usize,
}
```

À terme, cette fonction devrait lire les mêmes données que le panel, mais sans
construire tout `PanelVm`. Cela suffit à rendre `panel_focus` testable sans
tirer le rendu complet.

### Assertion : refresh workspace mélange I/O, store et état UI

Localisation :

- `AppAction::Store` invalide d'abord l'état dans le reducer
  `store.rs:128` ;
- `App::update` traite ensuite `Store(event)` à part dans `runtime.rs:139` ;
- `handle_store_event` lance une tâche ou tombe en synchrone dans
  `workspace_refresh.rs:8` ;
- `queue_store_task` construit `TaskSpec::refresh_git_overlay` ou
  `TaskSpec::reload_store` dans `workspace_refresh.rs:15` ;
- application d'un catalogue : `workspace_refresh.rs:42` ;
- application d'un reload : `workspace_refresh.rs:54` ;
- application d'un refresh git : `workspace_refresh.rs:72` ;
- recalcul du filtre actif : `workspace_refresh.rs:87` ;
- recalcul de l'usage lens : `workspace_refresh.rs:99`.

Friction organisationnelle :

Ce workflow est forcément transversal. La friction vient plutôt de l'ordre
implicite : watcher roots, options de recherche, modèles navigation, filtre,
usage lens, résultats visibles, sélection, vue, status. L'ordre est encodé comme
suite d'appels impératifs.

Approche lean :

Ne pas commencer la refactor par ce workflow. Ajouter d'abord des tests de
contrat sur l'ordre observé. Ensuite seulement, extraire une fonction
`apply_workspace_snapshot_change(...)` ou `refresh_ui_after_store_change(...)`
qui garde l'ordre dans un seul endroit.

### Assertion : la VM dépend de `App`, pas d'un contrat de lecture minimal

Localisation :

- `ExplorerVm::from_app` lit directement `App` dans `explorer/vm.rs:106` ;
- `search_vm`, `primary_nav_vm` et `search_popup_vm` continuent à lire `App`
  dans `explorer/vm.rs:137`, `explorer/vm.rs:155`, `explorer/vm.rs:209` ;
- `primary_nav_vm` lit aussi `app.store().stats()` dans `explorer/vm.rs:229` ;
- les panels lisent `App` et `WorkspaceStore` directement dans
  `panel_content.rs:19`, `panel_content.rs:67`, `panel_content.rs:168`.

Friction organisationnelle :

Le rendu final `render_shell(frame, area, &vm)` est bien pur, mais la
construction de VM est couplée au contrôleur `App`. Les tests de présentation
doivent donc instancier ou simuler plus que nécessaire.

Approche lean :

Introduire un contexte de lecture explicite, pas une abstraction ambitieuse :

```rust
struct ExplorerVmContext<'a> {
	state: &'a AppState,
	navigation: &'a NavigationState,
	workspace: &'a dyn IndexStore,
}
```

Puis migrer `from_app` progressivement vers `from_context`. `from_app` peut
rester comme adaptateur transitoire.

### Synthèse des frictions de localisation

Les problèmes principaux ne sont pas des manques d'abstraction. Ce sont des
responsabilités simples placées trop loin les unes des autres :

- le reducer connaît le message mais pas toujours la décision finale ;
- `Effect` mélange runtime et poursuite de transition ;
- `App` contient les règles de parcours, l'orchestration, les accès workspace et
  quelques side-channels runtime ;
- la navigation a un reducer propre, mais ses effets utilisateur observables
  sont complétés ailleurs ;
- le panel focus dépend du `PanelVm`, donc d'une structure de présentation ;
- le refresh workspace encode un ordre important sous forme de suite d'appels.

Le refactor devrait donc rester local et lisible :

1. choisir un workflow ;
2. localiser son contrat dans une fonction nommée ;
3. garder les inputs explicites (`state`, `workspace`, `navigation`) ;
4. retourner des actions simples ;
5. supprimer seulement les indirections devenues inutiles.

À éviter :

- rentrer tout `WorkspaceStore` dans `AppState` pour satisfaire une pureté TEA ;
- créer un framework générique de workflow ;
- introduire des traits avant d'avoir deux usages réels ;
- déplacer du code sans réduire le nombre de lieux à lire pour comprendre une
  action.

## Plan d'action jusqu'au refactor complet

Le refactor doit avancer par vertical slices. Chaque slice doit réduire le nombre
de fichiers à lire pour comprendre un comportement utilisateur. Si une étape
déplace du code sans clarifier le chemin, elle est à reprendre.

### Règles de méthode

- Ne traiter qu'un workflow à la fois.
- Avant de modifier un workflow, écrire son contrat observable en test.
- Garder les fonctions de décision proches du module métier concerné.
- Passer explicitement les lectures nécessaires (`state`, `navigation`,
  `workspace`) au lieu de passer `&App` par réflexe.
- Retourner des actions simples plutôt qu'un objet abstrait générique.
- Ne créer un trait que lorsqu'il supprime une dépendance concrète à `App` ou
  facilite un test réel.
- Garder `WorkspaceStore` hors de `AppState` tant qu'aucun besoin prouvé ne
  justifie ce déplacement.
- Après chaque slice, supprimer l'effet ou le handler devenu inutile.
- Le critère principal est la lisibilité du chemin, pas la conformité à un
  pattern théorique.

### Gate de validation

Pour chaque slice :

1. tests ciblés du workflow modifié ;
2. `cargo test -p code-moniker ui::... --lib` quand les tests sont en unit ;
3. `cargo test -p code-moniker --test cli_e2e ...` seulement si le comportement
   terminal/CLI est touché ;
4. `cargo fmt --all -- --check` ;
5. `cargo check --workspace --exclude code-moniker-pg --all-targets`.

Lancer `cargo arch-check` si la slice déplace des frontières de modules, change
les imports structurants ou introduit un nouveau module partagé.

### Étape 0 : état des lieux mesurable

But : figer les frictions actuelles avant déplacement.

Inventaire initial :

| Mesure | État observé |
| --- | --- |
| `Effect` runtime | `ShowView`, `Quit`, `DebounceHeaderSearch`, `CopyPanelSnapshot`, `RunCheck` |
| `Effect` transition UI | `ApplyHeaderSearch`, `CycleHeaderSearchSelector`, `ToggleHeaderSearchSelection`, `FocusUsages`, `ToggleChangeMode`, `Navigation`, `ToggleFocusRegion`, `PanelMove`, `PanelHome`, `PanelEnd`, `ToggleSelectedNode`, `OpenSelectedNode`, `CloseNodeOrClearScope` |
| Handlers `App` multi-responsabilités | `apply_header_search`, `clear_filter_with_focus`, `focus_usages`, `close_usage_lens`, `toggle_change_mode`, `toggle_change_usages`, `dispatch_navigation`, `apply_workspace_*` |
| `explorer::active_panel(self)` hors rendu | `panel_focus.rs`: focus panel, mouvement panel, home/end panel, copie snapshot |
| Lectures `store()` depuis UI | `app/*` pour orchestration, `explorer/vm.rs` pour stats/détails, `explorer/panel_content.rs` pour contenus de panel |

Actions :

- Lister tous les variants `Effect` et les classer en deux groupes :
  `runtime` ou `transition UI`.
- Lister tous les handlers `App` qui appellent plus d'un des éléments suivants :
  `dispatch_shell`, `dispatch_navigation`, `refresh_results`,
  `sync_contextual_view`, `set_status`, `queue_task`.
- Lister les appels à `explorer::active_panel(self)` hors rendu.
- Lister les appels à `store()` depuis `ui/explorer` et `ui/app`.

Sortie attendue :

- une courte table dans cette doc ou une issue dédiée ;
- aucun changement fonctionnel.

Validation :

- `git diff --check`.

### Étape 1 : classifier les effets

But : rendre explicite ce qui est runtime et ce qui est transition UI.

Actions :

- Renommer conceptuellement les effets runtime dans le code ou dans une doc
  locale : debounce, tâche async, clipboard, quit.
- Ajouter un commentaire court sur l'enum `Effect` indiquant quels variants sont
  temporaires et devraient migrer vers des décisions UI.
- Ne pas déplacer encore tous les variants.

Sortie attendue :

- le lecteur sait immédiatement que `ApplyHeaderSearch`, `FocusUsages`,
  `ToggleChangeMode`, `PanelMove`, `Navigation`, `OpenSelectedNode` ne sont pas
  des effets runtime définitifs.

Validation :

- tests existants ;
- `cargo fmt --all -- --check`.

### Étape 2 : vertical slice recherche header

But : localiser le contrat "appliquer une recherche".

Actions :

- Ajouter des tests couvrant :
  - recherche vide ;
  - recherche avec premier match ;
  - debounce périmé ;
  - filtre lang/kind.
- Extraire une fonction locale dans `header_search.rs` qui calcule une décision
  d'application de recherche.
- Garder un type de retour simple. Exemple :

```rust
struct HeaderSearchDecision {
	shell: Vec<ShellAction>,
	navigation: Vec<NavigationAction>,
	select: Option<DefLocation>,
	status: Option<String>,
	sync_contextual_view: bool,
}
```

- Faire appliquer cette décision par `App::apply_header_search`.
- Une fois stable, supprimer `Effect::ApplyHeaderSearch` si le reducer peut
  appeler directement la décision, ou réduire ce variant à un adaptateur très
  temporaire avec un TODO explicite.
- Ne pas généraliser `HeaderSearchDecision` en `UiDecision` tant qu'un second
  workflow ne réutilise pas la forme.

Sortie attendue :

- le comportement de recherche est compréhensible depuis `header_search.rs` ;
- `state.rs` ne porte plus qu'une décision de routage simple pour les touches ;
- les tests décrivent le contrat utilisateur.

Validation :

- test ciblé header search ;
- `cargo test -p code-moniker ui::app::... --lib` ou groupe équivalent ;
- `cargo fmt --all -- --check`;
- `cargo check --workspace --exclude code-moniker-pg --all-targets`.

### Étape 3 : navigation outcome

But : localiser les compléments systématiques de navigation.

Actions :

- Introduire un petit résultat de dispatch navigation, par exemple :

```rust
struct NavigationDispatchOutcome {
	changed: bool,
	selection_changed: bool,
	notice: TreePaneNotice,
}
```

- Faire produire ce résultat par `App::dispatch_navigation` ou une fonction
  dédiée proche de `navigation.rs`.
- Regrouper les règles :
  - sélection changée -> reset panel navigation ;
  - navigation changée -> sync contextual view si politique contextuelle ;
  - notice open/close -> status.
- Éviter de fusionner `NavigationAction` dans `AppAction` à cette étape.

Sortie attendue :

- `toggle_selected_nav`, `open_selected_nav`, `close_selected_nav` ne dupliquent
  plus la lecture de `last_notice` et la logique status ;
- le contrat post-navigation tient dans un résultat nommé.

Validation :

- tests navigation existants ou nouveaux tests unitaires sur sélection/reset ;
- `cargo arch-check` si les modules `ui/store` et `ui/app` changent de frontière.

### Étape 4 : usage lens et mode changements

But : séparer deux parcours utilisateur aujourd'hui voisins.

Actions :

- Créer un module `change_mode.rs` si le découpage reste naturel après lecture :
  `toggle_change_mode`, `toggle_change_usages`, status associés.
- Garder `usage_lens.rs` pour :
  - ouvrir une lens ;
  - fermer une lens ;
  - recalculer une lens après reload.
- Ajouter des tests :
  - `u` sans sélection ;
  - `u` avec sélection ;
  - `u` ferme une lens ouverte ;
  - `u` en mode changements alterne diff/usages ;
  - `d` entre en mode changements.
- Extraire au besoin une petite décision `UsageLensDecision`, mais seulement si
  cela évite de tester via `App` complet.

Sortie attendue :

- le module `usage_lens` ne cache plus le mode changements ;
- le raccourci `u` reste facile à comprendre car son multiplexage est nommé.

Validation :

- tests ciblés usage/change ;
- `cargo fmt --all -- --check`;
- `cargo check --workspace --exclude code-moniker-pg --all-targets`.

### Étape 5 : panel navigation sans construire le panel complet

But : supprimer la dépendance de contrôle vers `PanelVm`.

Actions :

- Ajouter un read model minimal :

```rust
struct ActivePanelNav {
	component: ComponentId,
	navigation_len: usize,
}
```

- Remplacer les appels à `explorer::active_panel(self)` dans `panel_focus.rs`
  par une fonction `active_panel_nav(...)`.
- Garder `copy_panel_snapshot` sur le chemin `PanelVm`, car il a réellement
  besoin du rendu texte du panel.
- Ajouter des tests :
  - focus panel initialise la sélection ;
  - mouvement borne la sélection ;
  - panel vide scrolle ;
  - changement de composant reset sélection/scroll.

Sortie attendue :

- `panel_focus.rs` ne construit plus une VM de rendu pour prendre une décision
  d'état ;
- seule la copie clipboard construit encore le snapshot complet.

Validation :

- tests panel focus ;
- `cargo fmt --all -- --check`;
- `cargo check --workspace --exclude code-moniker-pg --all-targets`.

### Étape 6 : VM context explicite

But : réduire la dépendance de la construction VM à `App`.

Actions :

- Introduire un contexte transitoire :

```rust
struct ExplorerVmContext<'a> {
	state: &'a AppState,
	navigation: &'a NavigationState,
	workspace: &'a dyn IndexStore,
}
```

- Garder `ExplorerVm::from_app` comme adaptateur temporaire.
- Migrer progressivement :
  - search VM ;
  - nav VM ;
  - panels simples ;
  - panels workspace-heavy.
- Ne pas convertir toute la VM en une fois si cela produit un gros diff.

Sortie attendue :

- les fonctions VM lisent un contexte déclaratif ;
- `App` n'est plus le contrat de lecture implicite du rendu.

Validation :

- tests VM/render existants ;
- tests ciblés si ajoutés ;
- `cargo arch-check` recommandé.

### Étape 7 : refresh workspace

But : rendre l'ordre post-refresh explicite sans cacher l'I/O.

Précondition :

- recherche, navigation outcome et usage lens sont stabilisés.

Actions :

- Ajouter des tests de contrat sur reload/git refresh :
  - filtre header recalculé ;
  - usage lens recalculée ;
  - mode change sélectionne le premier changement si nécessaire ;
  - tâche périmée ignorée.
- Extraire une fonction nommée autour de l'ordre post-refresh, par exemple
  `refresh_ui_after_store_change`.
- Garder les tâches async dans le runtime (`queue_task`, `TaskRunner`).
- Garder la mutation réelle du `WorkspaceStore` hors reducer.

Sortie attendue :

- le workflow transversal reste transversal, mais son ordre est lisible dans un
  endroit.

Validation :

- tests workspace refresh ;
- `cargo fmt --all -- --check`;
- `cargo check --workspace --exclude code-moniker-pg --all-targets`;
- `cargo arch-check`.

### Étape 8 : réduire ou supprimer les effets de transition UI

But : faire de `Effect` un ensemble de commandes runtime.

Précondition :

- les workflows précédents ont chacun une décision localisée.

Actions :

- Supprimer les variants qui ne représentent plus du runtime :
  - `ApplyHeaderSearch` ;
  - `CycleHeaderSearchSelector` ;
  - `ToggleHeaderSearchSelection` ;
  - `FocusUsages` ;
  - `ToggleChangeMode` ;
  - `Navigation` ;
  - `ToggleFocusRegion` ;
  - `PanelMove` ;
  - `PanelHome` ;
  - `PanelEnd` ;
  - `ToggleSelectedNode` ;
  - `OpenSelectedNode` ;
  - `CloseNodeOrClearScope`.
- Garder ou renommer les vrais runtime effects :
  - quit ;
  - debounce ;
  - run check ;
  - clipboard ;
  - async task.
- Simplifier `App::apply_effect`.

Sortie attendue :

- `Effect` ne ré-entre plus dans le controller pour finir une transition UI ;
- `runtime.rs` lit comme un interpréteur runtime.

Validation :

- suite UI ciblée ;
- `cargo fmt --all -- --check`;
- `cargo check --workspace --exclude code-moniker-pg --all-targets`;
- `cargo arch-check`.

### Étape 9 : décider du sort de `NavigationAction` et des générations

But : supprimer les mécanismes restants qui n'ont plus d'usage clair.

Actions :

- Vérifier si `NavigationAction` peut rester un sous-reducer isolé et simple.
- Si le chemin est encore double et coûteux à comprendre, intégrer
  `NavigationAction` dans le dispatch app principal.
- Auditer les compteurs `generation` :
  - conserver les époques de work async ;
  - supprimer les générations non lues ;
  - ou les brancher explicitement sur un redraw conditionnel si cela vaut le
    coût.

Sortie attendue :

- plus de bump manuel non consommé ;
- navigation soit clairement autonome, soit clairement intégrée.

Validation :

- tests navigation ;
- `cargo arch-check`;
- short gate complet :
  - `cargo fmt --all -- --check`
  - `cargo check --workspace --exclude code-moniker-pg --all-targets`
  - tests UI ciblés

### Étape 10 : nettoyage final

But : finir le refactor au lieu de laisser des adaptateurs permanents.

Actions :

- Supprimer les adaptateurs temporaires :
  - `ExplorerVm::from_app` si plus nécessaire ;
  - TODO sur effects temporaires ;
  - helpers `App` qui ne font plus que relayer.
- Vérifier que chaque workflow principal tient dans un module évident.
- Mettre à jour cette doc :
  - marquer les étapes terminées ;
  - retirer les constats devenus faux ;
  - documenter la nouvelle topologie.
- Lancer la gate pré-review :
  - `cargo fmt --all -- --check`
  - `cargo check --workspace --exclude code-moniker-pg --all-targets`
  - tests ciblés UI
  - `cargo arch-check`

Sortie attendue :

- un lecteur peut suivre chaque action utilisateur majeure en partant d'un
  module principal, sans traverser une chaîne reducer -> effect -> App handler
  pour de la logique purement UI ;
- les abstractions introduites ont chacune un usage concret et testable.

## État d'implémentation

Refactor appliqué :

- étapes 0-1 : inventaire initial documenté et `Effect` réduit aux commandes
  runtime (`ShowView`, `Quit`, debounce, clipboard, check) ;
- étape 2 : recherche header localisée dans `HeaderSearchDecision`, avec tests
  recherche vide, premier match, debounce périmé et filtres lang/kind ;
- étape 3 : `NavigationDispatchOutcome` regroupe changement de sélection et
  notice open/close ;
- étape 4 : `change_mode.rs` porte le mode changements, `usage_lens.rs` reste
  centré sur la lens, avec tests usage/change ciblés ;
- étape 5 : `panel_focus.rs` lit `ActivePanelNav` au lieu de construire le panel
  complet ; le snapshot clipboard reste sur `PanelVm` ;
- étape 6 : `ExplorerVm::from_app` est un adaptateur vers
  `ExplorerVm::from_context`, les VM search/nav lisent un contexte explicite ;
- étape 7 : l'ordre post-refresh est regroupé dans
  `refresh_ui_after_store_change` ;
- étape 8 : les effects de transition UI ont été supprimés et les messages
  concernés sont orchestrés directement côté runtime/app.

Décision d'architecture :

- La règle `ui-elm-orchestration-boundary` autorise désormais
  `ui/app/runtime.rs` à importer `Msg`, car la suppression des effects UI fait
  du runtime l'orchestrateur explicite de ces transitions autour du reducer.

Reste volontaire :

- `NavigationAction` reste un sous-reducer isolé : après `NavigationDispatchOutcome`,
  son coût de compréhension est acceptable.
- Les compteurs `generation` restent en place : ils servent encore aux redraws
  shell/navigation et aux époques de travail async ; leur suppression demanderait
  une instrumentation dédiée.
- `ExplorerVm::from_app` reste comme adaptateur public de transition pour ne pas
  déplacer tout le rendu/panel dans le même diff.
