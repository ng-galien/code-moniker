# Étude pour une city map logicielle dans Code Moniker

Date : 22 septembre 2026
Statut : étude de conception, aucune implémentation proposée comme acquise

Note de périmètre : la recommandation initiale fondée sur les Project Views
ci-dessous a été écartée. L'implémentation actuelle dans `packages/city-map`
part exclusivement de l'index et calcule ses regroupements automatiquement.
Ce document est conservé comme recherche historique, pas comme spécification.

## Résumé exécutif

Une software city est utile lorsqu'elle sert de carte d'orientation et de
détection de motifs. Elle devient rapidement inutile lorsqu'elle essaie de
dessiner tout le graphe, d'encoder trop de métriques à la fois ou de faire
passer une géographie calculée pour une vérité architecturale.

La littérature et les outils existants conduisent à cinq décisions fortes pour
Code Moniker :

1. La position horizontale doit être stable et explicable. Elle ne doit pas
   changer avec la métrique active, le filtre ou un petit changement du graphe.
2. Les districts doivent venir en priorité des frontières architecturales
   déclarées dans les Project Views, pas de la seule arborescence des fichiers.
3. Les relations ne doivent jamais être affichées exhaustivement. La vue
   globale encode une partie de leur direction dans le placement ; les lignes
   explicites sont réservées à la sélection, aux agrégats et aux anomalies.
4. Une ville répond à une question à la fois. Ownership, couplage, conformité,
   changement et impact sont des lentilles synchronisées sur une même
   géographie, pas cinq effets graphiques superposés.
5. La ville reste une surface d'orientation. La preuve exacte appartient au
   Cockpit, aux chemins du graphe, aux règles et au code source.

La recommandation est un **cadastre architectural 2,5D**, complété par trois
lentilles synchronisées : ownership/isolation, flux/couplage et
conformité/proposition. Le premier prototype doit partir d'une Project View
existante, montrer explicitement la zone non attribuée, puis permettre de
sélectionner un district et d'expliquer ses traversées de frontière.

## 1. Objet de l'étude

L'objectif n'est pas de produire une ville décorative ni de reprendre le
prototype Three.js existant. Il s'agit de déterminer comment une représentation
spatiale peut aider à répondre à des questions réelles :

- où sont les frontières et les propriétaires déclarés ;
- quels flux traversent une frontière ;
- quelle isolation est effective ou violée ;
- où se trouvent les cycles et dépendances inversées ;
- quelle zone serait affectée par une modification ;
- comment comparer l'architecture actuelle à une proposition ;
- comment conserver une carte mentale au fil des révisions.

### Une définition utile de « 5D »

Il ne faut pas comprendre 5D comme cinq métriques simultanées. Une définition
opérationnelle est :

| Dimension | Signification |
|---|---|
| X et Z | géographie architecturale stable |
| Y | une seule métrique de hauteur active |
| Temps | révision, évolution ou état de proposition |
| Lentille | question analytique active : structure, flux, conformité, impact |

Les relations constituent une surcouche filtrée. La couleur, la texture et les
formes sont des canaux limités, documentés par une légende permanente.

## 2. État de l'art

### CodeCity

CodeCity a établi la métaphore classique : packages comme quartiers, classes
comme bâtiments, nombre de méthodes comme hauteur et nombre d'attributs comme
emprise. Son étude contrôlée a mesuré une exactitude supérieure de 24,26 % et
un temps inférieur de 12,01 % par rapport à Eclipse complété par des tableaux.
L'avantage est surtout visible pour les questions d'ensemble, la dispersion et
l'impact. Pour trouver une valeur précise, un tableau peut rester plus rapide.

Sources : [présentation de CodeCity](https://wettel.github.io/codecity.html),
[étude contrôlée](https://wettel.github.io/download/Wettel11a-icse.pdf).

### CodeCharta

CodeCharta est aujourd'hui la référence pratique la plus complète. Chaque
fichier devient un bâtiment et chaque dossier un district dans une treemap 3D.
L'utilisateur choisit séparément les métriques de surface, hauteur, couleur et
relations. L'outil propose recherche, inspection, légende, scénarios enregistrés
et comparaison de deux cartes.

Ses points forts sont la maturité de l'interaction et la clarté des mappings.
Sa limite pour Code Moniker est structurelle : la géographie suit d'abord les
dossiers. Elle ne représente pas naturellement une frontière architecturale
déclarée qui traverse plusieurs répertoires.

Sources : [métaphore et navigation](https://codecharta.com/docs/visualization/user-controls/map/),
[métriques visuelles](https://codecharta.com/docs/visualization/user-controls/metrics/),
[comparaison](https://codecharta.com/docs/visualization/user-controls/compare/).

### EvoStreets

EvoStreets représente la hiérarchie par un réseau de rues et les classes par des
parcelles. Sa contribution majeure est la stabilité temporelle : l'ordre et le
côté d'une parcelle sont conservés, et un élément supprimé peut laisser un
emplacement vide. Une étude sur plusieurs versions a donné la meilleure
stabilité à EvoStreets pour 82 % des transitions observées.

Le compromis est important : les restructurations fréquentes produisent des
rues longues, des terrains abandonnés et une carte moins compacte. C'est un bon
modèle pour préserver la carte mentale, pas un argument pour conserver tout
espace vide indéfiniment.

Source : [Understanding software evolution with software cities](https://journals.sagepub.com/doi/10.1177/1473871612438785).

### Software Cartography

Cette approche place les artefacts selon la similarité de leur vocabulaire,
puis applique plusieurs couches thématiques à la même géographie. Elle répond
directement au problème du placement arbitraire : la proximité signifie une
similarité conceptuelle mesurée.

Pour Code Moniker, cette géographie serait intéressante comme lentille
exploratoire. Elle ne peut toutefois pas remplacer les frontières déclarées :
deux éléments lexicalement proches ne partagent pas nécessairement le même
propriétaire ou les mêmes contraintes.

Source : [Software cartography](https://arxiv.org/abs/1209.5490).

### CodeMetropolis et Code Park

CodeMetropolis utilise Minecraft ; Code Park représente les classes comme des
pièces dont les murs portent le code. Ces travaux montrent la force pédagogique
et démonstrative d'un espace immersif.

Ils montrent aussi sa limite opérationnelle. Dans l'étude Code Park, les
participants ont apprécié l'expérience, surtout les débutants, mais Visual
Studio était significativement plus rapide sur plusieurs tâches. Les
transitions animées longues et la navigation à la première personne pénalisent
le travail quotidien.

Sources : [CodeMetropolis](https://github.com/codemetropolis/CodeMetropolis),
[étude Code Park](https://arxiv.org/abs/1708.02174).

### ExplorViz et DynaCity

ExplorViz combine des paysages hiérarchiques, une ville 3D et des appels
dynamiques agrégés. Les détails n'apparaissent que dans le focus courant. Les
expériences publiées montrent que les abstractions hiérarchiques augmentent la
justesse sans coût temporel significatif. DynaCity rapporte également de
meilleurs résultats lorsque les appels sont agrégés et que couleur et activité
guident l'attention.

L'enseignement majeur est le suivant : une relation dynamique n'est lisible que
si elle est agrégée, contextualisée et limitée à la question active.

Sources : [présentation ExplorViz](https://explorviz.dev/1-about/),
[étude contrôlée](https://www.sciencedirect.com/science/article/pii/S0950584916301185).

### Layered Software City

Cette approche utilise la direction dominante des dépendances pour placer les
éléments sur des couches et ne dessine explicitement que les cycles et
violations. Dans la pré-étude, l'affichage de 19 732 arcs empêchait les
participants de répondre aux questions. Avec la représentation en couches, la
médiane de bonnes réponses a atteint 100 %, contre 57 % pour une treemap
améliorée.

Le placement devient lui-même porteur de sens. Il faut néanmoins distinguer un
rang calculé d'une couche architecturale déclarée : un calcul topologique ne
doit pas être présenté comme une intention du projet.

Source : [Static and Dynamic Dependency Visualization in a Layered Software City](https://link.springer.com/article/10.1007/s42979-022-01404-6).

### M3triCity

M3triCity calcule un layout sur l'histoire afin que chaque artefact conserve une
position fixe. Une timeline permet de voir croissance, suppression et
déplacement. Cette technique convient bien à l'évolution et aux refactorings,
mais réserve l'emprise maximale historique et peut donc produire beaucoup
d'espace inutilisé.

Source : [M3triCity](https://www.inf.usi.ch/lanza/Downloads/Pfah2020a.pdf).

### Synthèse de la recherche

Une revue systématique portant sur 105 publications rappelle que les besoins se
répartissent entre structure, comportement et évolution, et que l'adoption des
visualisations reste limitée. Une ville générique ne répond donc pas à tous les
besoins : la question de l'utilisateur doit décider du thème affiché.

Source : [A Systematic Literature Review of Modern Software Visualization](https://arxiv.org/abs/2003.00643).

## 3. Échecs récurrents

### Le graphe spaghetti

Afficher chaque relation ne révèle pas le système ; cela produit une masse de
lignes qui cache les bâtiments et les relations importantes. L'edge bundling
réduit le bruit mais ne résout pas la question sémantique : pourquoi cette
relation est-elle affichée ?

La carte doit avoir trois niveaux :

1. au niveau global, le placement exprime la direction ou l'appartenance ;
2. sur sélection, les relations incidentes sont agrégées ;
3. en diagnostic, seules les violations, cycles ou chemins demandés sont
   matérialisés.

Le compteur `relations affichées / relations totales` et le critère de sélection
doivent rester visibles.

### La perte de carte mentale

Une treemap recalculée après un changement de métrique, de filtre ou de version
déplace les quartiers. L'utilisateur ne reconnaît plus la ville. La position
doit donc être indépendante des métriques et versionnée par une identité de
layout.

### L'occlusion 3D

Une caméra libre et des bâtiments très hauts cachent les éléments. La vue par
défaut doit être orthographique ou légèrement isométrique, avec rotation
limitée, retour au nord, mini-carte et zoom sémantique. La hauteur doit pouvoir
être comprimée ou neutralisée.

### Le réalisme décoratif

Textures, mobilier, ombres lourdes et détails urbains consomment des canaux
visuels sans ajouter d'information. La métaphore doit rester schématique.

### L'arbitraire des métriques

Une hauteur n'a de sens que si la question, la métrique, l'unité, l'échelle et
les données absentes sont explicites. Les thèmes prédéfinis doivent précéder les
réglages libres.

### La confusion entre orientation et preuve

Une city map indique où regarder. Elle ne remplace pas les nombres exacts, les
relations symboliques, le code source ou une règle exécutable. Chaque sélection
doit offrir un passage direct vers ces preuves.

## 4. Diagnostic de l'ancien Urban Plan

L'ancien prototype avait deux bonnes décisions :

- séparation Capture → Scene IR → Renderer ;
- volonté de conserver une géographie stable lorsque seule la métrique change.

Mais son vocabulaire visuel ne correspondait pas encore au produit.

### Problèmes constatés

- Les quartiers suivent `crate → src/tests → module`. Ils représentent la
  hiérarchie d'identité, pas l'architecture déclarée.
- `defs` contrôle à la fois la surface et la hauteur. La même information est
  donc affichée deux fois.
- L'ordre des crates dépend du couplage courant. Un changement d'arête peut
  déplacer la ville.
- Les couleurs sont prises dans une palette selon l'ordre de placement, sans
  sémantique stable.
- Toutes les catégories de relations sont fusionnées dans les mêmes routes et
  leur direction n'est pas lisible.
- Le classement global des seize routes les plus lourdes privilégie le volume,
  pas la signification architecturale.
- Les contrôles règlent surtout la géométrie : taille de ville, largeur de rue,
  profondeur, emprise. Ils ne posent pas une question d'architecture.
- La sélection affiche seulement le type, `defs` et l'identité. Elle n'ouvre ni
  preuve source, ni règle, ni Cockpit.
- Le snapshot actuel contient des bâtiments, quartiers et routes, mais aucune
  représentation de l'ownership, de l'isolation, des violations ou des flux
  métier.

Références locales : [étude initiale](../../experiments/urban-plan/STUDY.md),
[layout](../../experiments/urban-plan/src/layout.ts),
[routeur](../../experiments/urban-plan/src/route.ts),
[configuration](../../experiments/urban-plan/src/config.ts).

## 5. Grammaire visuelle commune

Chaque canal reçoit une seule signification et toute valeur calculée est
distinguée d'une intention déclarée.

| Canal | Signification recommandée |
|---|---|
| Position horizontale | appartenance architecturale stable |
| Frontière du sol | frontière déclarée et ownership |
| Surface de parcelle | taille stable ou classe de taille |
| Hauteur | une métrique active |
| Couleur du sol | propriétaire ou catégorie de frontière |
| Couleur du bâtiment | valeur du thème actif |
| Texture ou halo | couverture, incertitude ou conformité |
| Largeur d'un faisceau | volume de relations |
| Motif du faisceau | type de relation |
| Chevrons | direction |
| Brèche de frontière | violation localisée |

L'isolation n'est pas une grande distance produite par un moteur de forces. Elle
se lit dans une frontière, ses portes, son nombre de traversées et ses
violations.

## 6. Trois concepts pour Code Moniker

### Concept A — Cadastre architectural

Une Project View est le territoire affiché. Chaque `ViewBoundaryDto` devient un
district clos. Les preuves de cette frontière déterminent les parcelles. Ce qui
n'est pas revendiqué reste dans une zone grise « non attribué ».

Les relations traversent la frontière par des portes. Une traversée admise est
un faisceau discret ; une violation devient une brèche rouge. La sélection d'un
district masque les relations non incidentes et présente ownership,
interdictions, entrées, sorties, violations, couverture et preuves.

Cette représentation est la plus fidèle à Code Moniker parce que la géographie
vient du témoignage du projet. Son risque est l'incomplétude des vues. Elle doit
donc montrer explicitement les éléments manquants, les chevauchements et le
taux de couverture.

### Concept B — Ville en terrasses de dépendances

Les composantes fortement connexes du graphe sont calculées, puis les groupes
acycliques reçoivent un rang topologique. Le rang devient l'altitude d'une
terrasse. Les dépendances normales descendent ; une dépendance interdite qui
remonte devient un pont rouge. Un cycle est un îlot annulaire sur une terrasse.

Cette vue répond très bien à « qui dépend de qui et dans quel sens ? ». Elle
doit toutefois afficher séparément le rang calculé et la couche déclarée. Un
petit changement du graphe peut modifier le rang ; les positions horizontales
doivent donc être stabilisées entre générations.

### Concept C — Atlas de villes synchronisées

Le même cadastre et la même caméra sont partagés par plusieurs vues :

1. ownership et isolation ;
2. flux et couplage ;
3. conformité et couverture ;
4. proposition et delta.

La sélection d'un élément le surligne partout. Les relations n'apparaissent que
dans la vue flux ; les autres vues ne montrent que de petits compteurs aux
frontières. Cette solution évite la surcharge et rend les comparaisons faciles,
au prix d'un écran plus dense et moins spectaculaire.

## 7. Recommandation

Le produit cible doit combiner le **cadastre architectural** et l'**atlas de
lentilles synchronisées**.

La grande vue conserve la qualité spatiale d'une city map. Les lentilles
empêchent de superposer ownership, flux, métriques, violations et propositions
dans une seule scène. La ville en terrasses peut devenir une lentille spécialisée
pour les dépendances, pas le layout général.

### MVP proposé

1. Sélectionner une Project View existante.
2. Construire les districts uniquement depuis ses frontières et preuves.
3. Afficher une zone non attribuée.
4. Conserver une géographie déterministe et versionnée.
5. Sélectionner un district pour afficher ses flux entrants et sortants,
   filtrables par type, avec couverture et omissions.
6. Activer une lentille conformité qui place les violations sur les frontières.
7. Ouvrir les preuves exactes dans le Cockpit ou le code.
8. Déplacer un seul agrégat dans une proposition et comparer l'état courant à
   la projection, avec la même caméra.

La première validation devrait utiliser une vue dont les propriétaires sont
explicites, puis une vue de linkage plus complexe.

### Proposition actuelle / cible

Deux cartes synchronisées utilisent les mêmes emplacements de base :

- état courant solide ;
- proposition translucide ;
- ajout avec contour ;
- retrait comme parcelle vide ;
- déplacement par trace fantôme ;
- compteurs avant/après pour traversées, violations et éléments non attribués.

La projection doit porter la mention : « conséquences calculées sur les
relations actuelles ». Elle ne prétend pas que le code refactoré compilera ni
que toutes les relations futures sont connues.

## 8. Données Code Moniker disponibles

Le moteur actuel fournit déjà une base plus riche que le snapshot Urban Plan :

- `identity.children` : hiérarchie agrégée et symboles navigables ;
- `identity.graph` : nœuds, relations agrégées, ports entrants/sortants,
  non-liés et couverture ;
- `metrics.coupling` : volumes, connexions, types et cibles ;
- `symbol.graph`, `graph.path`, `graph.corridor` : explication et drill-down ;
- Project Views : ownership, interdictions, règles, preuves et éléments
  manquants ;
- `rules.check` : violations localisées ;
- `resolution.audit` : distinction entre relations résolues, candidates,
  dynamiques, externes et non résolues ;
- `diff-impact.compare` : impact d'un changement réellement matérialisé.

Le Scene IR futur doit garder la provenance et la couverture, pas seulement les
coordonnées :

```text
SceneSnapshot
  generation, root, view_id, layout_id
  districts[]
    boundary_id, owns, forbids, evidence, missing, rect
  buildings[]
    identity, district_id?, stable_slot, metrics, provenance
  crossings[]
    from, to, kinds, count, coverage, rule_status
  violations[]
    rule_id, source, target?, severity, evidence
  proposal?
    moves[], projected_crossings[], projected_violations[]
```

## 9. Ce qu'il faut jeter et conserver

### À jeter comme logique produit

- treemap fondée sur `crate/src/module` comme architecture ;
- sériation par force de couplage ;
- couleur attribuée par ordre ;
- `defs` pour surface et hauteur ;
- pseudo-rues sans sémantique ;
- routeur géométrique Hanan comme pièce centrale ;
- top 16 global comme seule stratégie de relations ;
- réglages de dessin exposés comme réglages principaux ;
- districts déduits silencieusement des dossiers ;
- sélection sans preuve ni navigation analytique.

### À conserver

- séparation Capture → Scene IR → Renderer ;
- snapshot identifié par génération ;
- caméra orthographique, pan, zoom et picking ;
- budget et compteurs d'éléments omis ;
- identités Code Moniker comme clés ;
- stabilité du plan lorsque seule une métrique change.

## 10. Navigation et interaction

- Vue initiale orthographique ou isométrique légère, nord stable.
- Rotation limitée et optionnelle.
- Recherche, fil d'Ariane, mini-carte et retour à la vue d'ensemble.
- Zoom sémantique : territoire → district → agrégat → symbole.
- Transitions courtes, annulables, respectant la réduction de mouvement.
- Sélection persistante lors d'un changement de lentille ou de version.
- Légende permanente : question, métriques, unités, échelle, couverture.
- Aucun détail essentiel accessible uniquement au survol.

## 11. Protocole d'évaluation avant industrialisation

Le prototype doit être évalué sur des tâches, pas sur son aspect spectaculaire :

1. retrouver le composant d'entrée d'un flux ;
2. identifier une dépendance inversée ;
3. repérer un cycle ;
4. estimer la zone d'impact d'un changement ;
5. retrouver un artefact après changement de thème et de révision ;
6. comparer deux propositions d'architecture ;
7. ouvrir la preuve exacte dans le Cockpit.

Mesures :

- exactitude de la réponse ;
- temps ;
- nombre de manipulations caméra ;
- perte d'orientation ;
- nombre de relations consultées avant la preuve ;
- compréhension de la couverture et des omissions.

Le test doit comparer au minimum :

- Code Moniker sans city map ;
- ancien Urban Plan ;
- nouveau cadastre ;
- nouveau cadastre avec lentilles synchronisées.

## 12. Plan de reprise

### Étape 1 — Contrat visuel

Définir les questions, la grammaire, les invariants de stabilité et le contrat
du Scene IR. Aucun rendu Three.js avant validation de ce contrat sur des données
réelles.

### Étape 2 — Prototype 2D ou 2,5D minimal

Rendre une seule Project View, sa zone non attribuée et les traversées d'un
district sélectionné. Utiliser un renderer simple tant que la sémantique bouge.

### Étape 3 — Preuve et couverture

Relier les éléments au Cockpit, afficher la couverture et distinguer les
relations résolues, candidates et inconnues.

### Étape 4 — Lentilles synchronisées

Ajouter flux, conformité et comparaison sans modifier la géographie.

### Étape 5 — Proposition

Permettre un déplacement d'agrégat, calculer son delta sur les relations
actuelles et maintenir une séparation stricte entre proposition, code modifié et
validation réelle.

### Étape 6 — Validation utilisateur

Exécuter le protocole de tâches sur au moins deux dépôts structurellement
différents avant de choisir le niveau de finition graphique.

## Conclusion

Code Moniker ne doit pas construire une ville de fichiers. Il peut construire
une carte d'architecture dont chaque frontière, relation et anomalie renvoie à
une preuve indexée.

La direction recommandée est donc : **géographie déclarée et stable, hauteur
thématique, relations sélectives, temps explicite, lentilles synchronisées et
drill-down vers la preuve**. Cette direction conserve ce qui était juste dans
l'ancien Urban Plan tout en supprimant ses choix arbitraires.
