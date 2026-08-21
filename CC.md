# Clean Code rules and metric detection strategies

This document is a source catalog for future Code Moniker checks. It deliberately
separates two related but different bodies of work:

- Robert C. Martin's *Clean Code* heuristics are qualitative review guidance.
- Radu Marinescu's, and later Michele Lanza and Radu Marinescu's, detection
  strategies turn selected design disharmonies into metric predicates.

The entries below are not all suitable as hard build rules. A heuristic becomes a
useful check only when its required evidence exists, its thresholds are calibrated
for the project and language, and its false positives are understood. Smell checks
should normally begin as warnings.

## Robert C. Martin: the Chapter 17 catalog

Chapter 17, "Smells and Heuristics", is the book's explicit catalog. It contains
66 numbered entries in seven groups. Martin explains that the list includes
Fowler's smells as well as heuristics gathered during his own practice; attribution
to this chapter therefore does not mean that every underlying idea originated with
Martin. The list and numbering below follow the
[publisher's Chapter 17 page](https://www.oreilly.com/library/view/clean-code-a/9780136083238/chapter17.xhtml)
and were cross-checked against the
[published book contents](https://www.informit.com/store/clean-code-a-handbook-of-agile-software-craftsmanship-9780132639408)
and an independent
[chapter summary](https://github.com/thomasruegg/clean-code-summary#chapter-17-smells-and-heuristics).
The short intents are paraphrases, not quotations from the book.

### Comments

| ID | Heuristic | Review intent |
| --- | --- | --- |
| C1 | Inappropriate Information | Keep changeable metadata and administrative information outside source comments. |
| C2 | Obsolete Comment | Remove comments that no longer describe the code. |
| C3 | Redundant Comment | Do not restate behavior that the code already makes evident. |
| C4 | Poorly Written Comment | Make a necessary comment concise, precise, and readable. |
| C5 | Commented-Out Code | Delete inactive code and rely on version control to retain history. |

### Environment

| ID | Heuristic | Review intent |
| --- | --- | --- |
| E1 | Build Requires More Than One Step | Provide one clear command for producing the software. |
| E2 | Tests Require More Than One Step | Provide one clear command for running the test suite. |

### Functions

| ID | Heuristic | Review intent |
| --- | --- | --- |
| F1 | Too Many Arguments | Keep function signatures narrow and cohesive. |
| F2 | Output Arguments | Prefer returned values or receiver state to arguments mutated as output channels. |
| F3 | Flag Arguments | Avoid booleans that select multiple behaviors inside one function. |
| F4 | Dead Function | Remove functions that have no live use. |

### General

| ID | Heuristic | Review intent |
| --- | --- | --- |
| G1 | Multiple Languages in One Source File | Minimize embedded language fragments and keep boundaries explicit. |
| G2 | Obvious Behavior Is Unimplemented | Meet the unsurprising expectations created by an API or abstraction. |
| G3 | Incorrect Behavior at the Boundaries | Treat boundaries and edge cases as first-class behavior. |
| G4 | Overridden Safeties | Do not casually disable warnings, tests, validation, or other safeguards. |
| G5 | Duplication | Keep each piece of knowledge in one authoritative representation. |
| G6 | Code at Wrong Level of Abstraction | Keep high-level policy separate from low-level detail. |
| G7 | Base Classes Depending on Their Derivatives | Preserve the dependency direction from derived types toward their bases. |
| G8 | Too Much Information | Keep interfaces small and hide unnecessary data, methods, and constants. |
| G9 | Dead Code | Remove unreachable or unused branches, variables, and declarations. |
| G10 | Vertical Separation | Place closely related declarations near their use. |
| G11 | Inconsistency | Apply the same conventions to equivalent situations. |
| G12 | Clutter | Remove empty constructors, unused variables, and other non-contributing structure. |
| G13 | Artificial Coupling | Do not force unrelated concepts together for convenience. |
| G14 | Feature Envy | Move behavior toward the data or abstraction it primarily uses. |
| G15 | Selector Arguments | Replace arguments that select behavior with separate intentions or polymorphism. |
| G16 | Obscured Intent | Avoid dense expressions and incidental tricks that hide meaning. |
| G17 | Misplaced Responsibility | Put a decision or behavior in the component that owns it. |
| G18 | Inappropriate Static | Keep behavior on instances when it belongs to an object abstraction. |
| G19 | Use Explanatory Variables | Name meaningful intermediate results when they clarify an algorithm. |
| G20 | Function Names Should Say What They Do | Make a callable's behavior and contract apparent from its name. |
| G21 | Understand the Algorithm | Do not accept code that merely appears to work without understanding why. |
| G22 | Make Logical Dependencies Physical | Represent real ordering or data dependencies explicitly in code. |
| G23 | Prefer Polymorphism to If/Else or Switch/Case | Use dispatch when conditional type selection is the actual abstraction. |
| G24 | Follow Standard Conventions | Follow the team's and language's established structure and style. |
| G25 | Replace Magic Numbers with Named Constants | Give domain values names while leaving obvious formula literals alone. |
| G26 | Be Precise | Make contracts, choices, and exceptional cases explicit. |
| G27 | Structure over Convention | Encode important constraints structurally instead of relying only on discipline. |
| G28 | Encapsulate Conditionals | Give a complex condition an intention-revealing name or owner. |
| G29 | Avoid Negative Conditionals | Prefer positive predicates when they make control flow easier to read. |
| G30 | Functions Should Do One Thing | Keep each function at one coherent responsibility and abstraction level. |
| G31 | Hidden Temporal Couplings | Expose required operation order through data flow or API shape. |
| G32 | Do Not Be Arbitrary | Make similar design choices consistently and communicate their rationale. |
| G33 | Encapsulate Boundary Conditions | Centralize boundary calculations instead of scattering them. |
| G34 | Functions Should Descend Only One Level of Abstraction | Keep a function's statements at a consistent abstraction level. |
| G35 | Keep Configurable Data at High Levels | Place policy constants where the system's configuration is visible. |
| G36 | Avoid Transitive Navigation | Ask direct collaborators for work rather than traversing object chains. |

### Java

These three entries are Java-specific advice from the 2008 book. They must not
be generalized into language-neutral rules without reconsidering current language
and ecosystem conventions.

| ID | Heuristic | Review intent |
| --- | --- | --- |
| J1 | Avoid Long Import Lists by Using Wildcards | In the book's Java context, avoid enumerating a large package surface import by import. |
| J2 | Do Not Inherit Constants | Import or qualify constants rather than inheriting them through an interface. |
| J3 | Constants versus Enums | Prefer an enum when values form a closed named set with behavior or structure. |

### Names

| ID | Heuristic | Review intent |
| --- | --- | --- |
| N1 | Choose Descriptive Names | Use names that communicate purpose. |
| N2 | Choose Names at the Appropriate Level of Abstraction | Name the abstraction, not an incidental implementation detail. |
| N3 | Use Standard Nomenclature Where Possible | Reuse established domain, pattern, and language vocabulary. |
| N4 | Unambiguous Names | Make names distinguishable and hard to misread. |
| N5 | Use Long Names for Long Scopes | Increase name specificity as its scope grows. |
| N6 | Avoid Encodings | Do not embed type or scope metadata that tools already know. |
| N7 | Names Should Describe Side-Effects | Include non-obvious effects in a function's name or redesign its contract. |

### Tests

| ID | Heuristic | Review intent |
| --- | --- | --- |
| T1 | Insufficient Tests | Test every behavior whose failure would matter. |
| T2 | Use a Coverage Tool | Use coverage evidence to find paths that tests do not exercise. |
| T3 | Do Not Skip Trivial Tests | Keep inexpensive tests even when the behavior looks obvious. |
| T4 | An Ignored Test Is a Question about an Ambiguity | Treat disabled tests as unresolved design or requirement questions. |
| T5 | Test Boundary Conditions | Exercise edges, limits, and transitions explicitly. |
| T6 | Exhaustively Test Near Bugs | Expand tests around the neighborhood of a discovered defect. |
| T7 | Patterns of Failure Are Revealing | Analyze failures together because their distribution can expose the cause. |
| T8 | Test Coverage Patterns Can Be Revealing | Inspect which paths are and are not covered to find missing cases. |
| T9 | Tests Should Be Fast | Keep feedback fast enough that developers run tests routinely. |

## Marinescu: detection strategies

In his 2004 paper, Marinescu defines a detection strategy as a quantifiable
expression of a design rule. A strategy combines metrics in two stages:

1. **Filtering** selects suspicious values using absolute semantic thresholds,
   relative semantic thresholds, or statistical thresholds.
2. **Composition** combines the filtered sets with `and`, `or`, and `butnot`,
   corresponding to intersection, union, and set difference.

This is the conceptual ancestor that most directly matches Code Moniker's check
DSL. The definition, operators, and original examples come from
[Marinescu, *Detection Strategies: Metrics-Based Rules for Detecting Design Flaws* (ICSM 2004)](https://doi.org/10.1109/ICSM.2004.1357820);
an [author-uploaded full text](https://www.researchgate.net/publication/4104985_Detection_startegies_Metrics-based_rules_for_detecting_design_flaws)
is also available.

### Strategies named in the 2004 paper

The paper says that more than ten strategies had been defined and names the
following nine. This is not the same catalog as the later book.

| Strategy | Target | Main evidence described in the paper |
| --- | --- | --- |
| Shotgun Surgery | Class | Many incoming dependent methods spread across many client classes. |
| Wide Subsystem Interface | Subsystem | A subsystem exposes an unusually broad interface. |
| Feature Envy | Method | Foreign data access dominates local data access and is concentrated in few providers. |
| Misplaced Class | Subsystem | A class is more strongly coupled to another subsystem than to its own. |
| God Method | Method | An oversized, complex method concentrates too much behavior. |
| God Class | Class | High complexity, low cohesion, and excessive access to foreign data. |
| God Package | Subsystem | A package concentrates excessive size and dependency responsibility. |
| Data Class | Class | A class exposes data while providing little behavior. |
| Refused Bequest | Class | A subclass makes little use of the behavior or state it inherits. |

### Lanza and Marinescu's 2006 catalog

*Object-Oriented Metrics in Practice* organizes eleven design disharmonies into
identity, collaboration, and classification. The catalog and symbolic strategies
below follow the
[Springer book record](https://link.springer.com/book/10.1007/3-540-39538-5)
and the authors' openly available
[Chapter 5 on identity disharmonies](https://scg.unibe.ch/assets/files/47/mgckdeptwnul3k6wzkurax86qichae/Lanz06a-OOMIP-Chapter5.pdf).
Symbolic labels such as `FEW`, `HIGH`, and `VERY_HIGH` are intentional: the
method separates a strategy's shape from thresholds calibrated using empirical
distributions and generally accepted semantic limits.

#### Identity disharmonies

| Disharmony | Target | Published detection strategy, compact form |
| --- | --- | --- |
| God Class | Class | `ATFD > FEW AND WMC >= VERY_HIGH AND TCC < 1/3` |
| Feature Envy | Method | `ATFD > FEW AND LAA < 1/3 AND FDP <= FEW` |
| Data Class | Class | `WOC < 1/3 AND (((NOPA + NOAM > FEW) AND WMC < HIGH) OR ((NOPA + NOAM > MANY) AND WMC < VERY_HIGH))` |
| Brain Method | Operation | `LOC > HIGH(class LOC) / 2 AND CYCLO >= HIGH AND MAXNESTING >= SEVERAL AND NOAV > MANY` |
| Brain Class | Class | `(((brain_methods > 1) AND LOC >= VERY_HIGH) OR ((brain_methods = 1) AND LOC >= 2*VERY_HIGH AND WMC >= 2*VERY_HIGH)) AND WMC >= VERY_HIGH AND TCC < 1/2` |
| Significant Duplication | Clone or clone chain | `SEC > average operation LOC OR (SDC >= 2*(FEW+1)+1 AND SEC > FEW AND LB <= FEW)` |

#### Collaboration disharmonies

The coupling metric definitions and compact predicates are also documented in
Trifu and Marinescu's primary 2005 paper
[*Diagnosing Design Problems in Object Oriented Systems*](https://doi.org/10.1109/WCRE.2005.15),
where the high-dispersion branch is called **Extensive Coupling**.

| Disharmony | Target | Published detection strategy, compact form |
| --- | --- | --- |
| Intensive Coupling | Method | `((CINT > SHORT_MEMORY_CAP AND CDISP < 1/2) OR (CINT > FEW AND CDISP < 1/4)) AND MAXNESTING > SHALLOW` |
| Dispersed Coupling | Method | `CINT > SHORT_MEMORY_CAP AND CDISP >= 1/2 AND MAXNESTING > SHALLOW` |
| Shotgun Surgery | Method or class | High incoming change impact: `CM` is both absolutely high and among the highest values, while `CC` is high. The 2004 formulation is `CM > 10 AND top_20_percent(CM) AND CC > 5`. |

`CM` and `CC` are static ripple-risk proxies: they count calling/dependent methods
and the classes containing them. They do not by themselves prove historical
co-change.

#### Classification disharmonies

The same 2005 paper gives the `NAS/PNAS` Tradition Breaker composition and
explains why its statistical thresholds are symbolic rather than universal.

| Disharmony | Target | Published detection strategy, compact form |
| --- | --- | --- |
| Refused Parent Bequest | Class | `(((NProtM > FEW) AND BUR < 1/3) OR BOvR < 1/3) AND ((AMW > average OR WMC >= average) AND NOM > average)` |
| Tradition Breaker | Class | `(NAS >= average NOM AND PNAS >= 2/3) AND (AMW > average OR WMC >= VERY_HIGH) AND NOM >= HIGH`, together with `parent AMW > average AND parent NOM > HIGH/2 AND parent WMC >= VERY_HIGH/2`. |

The compact classification forms retain the authors' intent without pretending
that corpus-relative thresholds are universal constants. Implementations must
consult the book's metric definitions, especially for inherited, added, protected,
and accessor methods.

## Metric glossary

| Metric | Meaning |
| --- | --- |
| AMW | Average Method Weight, normally average method complexity in a class. |
| ATFD | Access To Foreign Data, distinct foreign attributes accessed directly or through accessors. |
| BOvR | Base-class Overriding Ratio, the share of a subclass's methods that override inherited behavior. |
| BUR | Base-class Usage Ratio, the share of inherited protected members used by the subclass. |
| CC | Changing Classes, distinct client classes that depend on the measured element. |
| CDISP | Coupling Dispersion, provider classes divided by distinct called methods. |
| CINT | Coupling Intensity, distinct external methods called. |
| CM | Changing Methods, distinct client methods that depend on the measured element. |
| CYCLO | Cyclomatic complexity. |
| FDP | Foreign Data Providers, classes owning foreign data accessed by a method. |
| LAA | Locality of Attribute Accesses, local attribute accesses divided by all attribute accesses. |
| LB | Line Bias, non-matching lines between neighboring exact clone fragments. |
| LOC | Lines of code, with the precise counting domain defined by the strategy. |
| MAXNESTING | Maximum nesting depth of control structures. |
| NAS | Number of Added Services not inherited or overridden from ancestors. |
| NOAM | Number Of Accessor Methods. |
| NOAV | Number Of Accessed Variables. |
| NOM | Number Of Methods. |
| NOPA | Number Of Public Attributes. Some extracted copies of the figure contain the transposition `NOAP`; the metric is `NOPA`. |
| NProtM | Number of protected members supplied by the parent. |
| PNAS | Proportion of Newly Added Services among a class's services. |
| SDC | Size of Duplication Chain, including close adapted gaps. |
| SEC | Size of Exact Clone. |
| TCC | Tight Class Cohesion, the proportion of method pairs sharing attribute use. |
| WMC | Weighted Method Count, the sum of method complexities. |
| WOC | Weight Of Class, the share of functional public interface rather than data exposure. |

## Code Moniker project assessment

### Current state

The live `smells` profile compiles 18 rules. All 18 use `plan=t0_local`; none
uses the workspace index. The repository nevertheless already has:

- two `workspace.symbol` consumer probes, `agent-private-type-single-consumer`
  and `agent-public-type-without-consumer`;
- nine `workspace.path` architecture rules;
- `workspace.group` support for symbol counts and line distributions.

The immediate problem is therefore not an empty rule pack. It is that the smell
pack remains local while the workspace engine has grown beside it.

All project rules and profiles belong in the root `.code-moniker.toml`. Do not
create a second Clean Code rules file.

### 1. Do now, without changing the DSL

These actions use capabilities that compile today.

| Rank | Action in `.code-moniker.toml` | Exact result |
| ---: | --- | --- |
| 1 | Add the two existing `agent-private-type-single-consumer` and `agent-public-type-without-consumer` rule IDs to `[profiles.smells].enable`. Do not copy the rules. | The smell run immediately gains full-index consumer evidence for low-yield private types and internally unused public surface. |
| 2 | Add a warning-severity `[[workspace.symbol.where]]` rule for production callables/types with unusually high `count(in_refs)`. Name it as an incoming ripple-volume signal, not as the complete Shotgun Surgery strategy. | A workspace-wide static precursor to Shotgun Surgery, with linkage coverage and without the file-local blind spot. |
| 3 | Add a local warning for excessive public surface on `rust.shape.type`, using filtered counts of public callable and value children. Keep it separate from `smell-large-type`, which measures total members. | Clean Code G8, Too Much Information, becomes an executable project rule. |
| 4 | Keep the existing `workspace.path` rules as architecture guardrails. Add new path rules only for an actual project boundary expressed by aliases; do not generate generic Clean Code paths. | G7, G13, and mandatory-boundary policies remain concrete project architecture rules with witnesses. |

The second action can only count all linked incoming references today. The rule
must therefore say exactly that. It must not claim distinct callers, distinct
client classes, call-only relations, or historical co-change.

### 2. Keep the current local rules, but label their fidelity correctly

| Current rule | Decision |
| --- | --- |
| `smell-long-parameter-list`, `smell-data-clumps-param-names`, `smell-vertical-layout` | Keep. They directly use evidence owned by the local source graph. |
| `smell-unwrap-in-production`, `smell-clone-reflex`, `smell-box-leak` | Keep as Rust project rules; they are not Marinescu strategies. |
| `smell-feature-envy-local` | Keep temporarily and retain `-local` in the name. Replace it when indexed `ATFD/LAA/FDP` can be expressed. |
| `smell-intensive-coupling-method` | Keep as an approximation. Its call count is filtered locally, but its distinct-owner collection is not filtered by the same relation predicate. Do not add Dispersed Coupling by mirroring this approximation. |
| `smell-god-type-local-metrics`, `smell-brain-method`, `smell-brain-class` | Keep as explicitly local proxies. Do not rename them as faithful Lanza/Marinescu implementations. |
| `smell-large-type`, method-size/fan-out distribution, RFC, caller concentration, low-cohesion module, helper satellites | Keep as project heuristics. They are useful independently of the published catalog. |

### 3. DSL evolutions, in implementation order

#### 1. Filtered workspace relation domains and bindings

This is the first missing operator family.

The workspace compiler currently rejects quantifiers as
`inventory.quantifier` and rejects `count(in_refs, filter)` /
`count(out_refs, filter)` as `linkage.filtered-count`. It only accepts an
unfiltered reference count compared with a literal.

Add workspace equivalents of the local relation model:

- filter incoming/outgoing edges by relation kind and endpoint facets;
- bind the current edge, its source, and its target;
- project distinct source/target symbols;
- project their owning type, module, package, source set, or root;
- apply `count`, `any`, `all`, `none`, `unique`, and collection algebra.

Implementation owners:

- expression model and parsing: `crates/check/src/check/expr/`;
- workspace capability compilation: `crates/check/src/check/workspace_eval.rs`;
- coverage-aware linked evaluation: `crates/check/src/check/workspace_eval/linkage.rs`.

This unlocks the missing structural core of Feature Envy, Intensive Coupling,
Dispersed Coupling, Shotgun Surgery, Misplaced Class, Wide Subsystem Interface,
and God Package.

#### 2. Workspace groups over computed relation metrics

`workspace.group.expr` currently supports only `count(member)` and descriptive
aggregates over `(member, lines)`. It cannot compute a distribution of incoming
callers, foreign-data providers, or coupled owners.

Allow a group to aggregate the scalars and collections produced by the first
evolution. This supplies:

- the top-value `CM` clause of Shotgun Surgery;
- package/subsystem coupling distributions;
- caller-owner concentration;
- workspace calibration for `FEW`, `MANY`, `HIGH`, and `VERY_HIGH`.

Implementation owner: `crates/check/src/check/workspace_eval/group/`, especially
`predicate.rs`. Preserve fail-closed coverage for unavailable member metrics.

#### 3. Numeric composition

The number grammar has literals, projections, counts, named metrics, and
aggregates, but no composition of numeric expressions. Add either a general
numeric algebra or a smaller ratio/fraction primitive after its zero-denominator
and unavailable-value semantics are defined.

This is required for `LAA`, `CDISP`, `WOC`, `BUR`, `BOvR`, and `PNAS`.

Implementation owners: `crates/check/src/check/expr/number.rs`, the local
evaluator, and the workspace trivalent evaluator.

#### 4. Owner/descendant roll-up on indexed relations

A workspace reference count is attached to the exact source or target symbol.
God Class and Brain Class need method activity rolled up to the containing type;
package strategies need type/member activity rolled up to a package or module.

Add explicit owner/descendant projection over indexed symbol and edge sets. Do
not make every count implicitly recursive.

Implementation owners: the workspace symbol inventory/linkage read index and
`crates/check/src/check/workspace_eval/linkage.rs`.

#### 5. `path` and `corridor` as graph values

`workspace.path` is currently a terminal rule root. It returns a verdict and a
minimal witness; its result cannot feed another DSL expression.

After the common symbol/edge collection model exists:

- expose reachability/path as a coverage-aware graph predicate with a witness;
- expose corridor as a bounded subgraph value;
- project corridor members and edges into the same collection operators;
- propagate incomplete traversal as `inconclusive`, never as an empty set.

The corridor computation belongs to the workspace snapshot path engine and is
already specified in [issue #13](https://github.com/ng-galien/code-moniker/issues/13).
The check DSL must consume that primitive rather than reimplement it in
`crates/check`.

#### 6. Indexed AST metrics

Add `CYCLO`, `MAXNESTING`, and `NOAV` to extractor output and the symbol
inventory. This replaces the current Brain Method size/fan-out proxy and
completes the flat-method guard used by the coupling strategies.

Implementation owners: `crates/core/src/lang/<lang>/` for extraction and the
workspace snapshot model for indexed storage.

#### 7. Inheritance member sets and bindings

Expose inherited/protected members, overrides, ancestor-relative added services,
and parent/child bindings. This is needed for Refused Parent Bequest and
Tradition Breaker; `dit` and `noc` alone are insufficient.

#### 8. New evidence producers, not DSL operators

- Significant Duplication requires a clone producer for `SEC`, `SDC`, and `LB`.
- Historical Shotgun Surgery and Divergent Change require Git/co-change data.
- Temporary Field requires data flow or reaching definitions.

Do not hide these missing data sources behind textual rule heuristics.

### 4. Resulting rule replacement order

| Published strategy | Project action |
| --- | --- |
| Feature Envy | Replace `smell-feature-envy-local` after evolutions 1, 3, and 4 provide indexed `ATFD/LAA/FDP`. |
| Intensive Coupling | Replace the local approximation after evolutions 1 and 3 provide filtered distinct callees and `CDISP`. |
| Dispersed Coupling | Add alongside the faithful Intensive Coupling rule; do not add the current unfiltered-owner approximation. |
| Shotgun Surgery | Add the volume-only warning now; replace it after evolutions 1 and 2 provide distinct `CM`, `CC`, and corpus top values. |
| God Class | Replace the local proxy after relation roll-up and indexed `ATFD/TCC` exist. |
| Brain Method | Replace the proxy after indexed AST metrics exist. |
| Brain Class | Compose the faithful Brain Method result with indexed owner roll-up and class metrics. |
| Data Class | Add only after public-data/accessor semantics and numeric composition exist. |
| Refused Parent Bequest / Tradition Breaker | Add after inheritance member bindings and ratios exist. |
| Significant Duplication | Add only after clone evidence exists. |

## Source and interpretation rules

1. Preserve the source name and ID when a check claims to implement a published
   heuristic.
2. Call a rule an approximation whenever its metrics or evidence differ from the
   published detection strategy.
3. Keep symbolic thresholds until a calibration corpus and counting semantics are
   recorded. Do not silently turn `HIGH`, `FEW`, or a percentile into an arbitrary
   universal number.
4. Distinguish a static dependency-risk proxy from repository history. In
   particular, Shotgun Surgery can be screened statically, while actual co-change
   requires version-control evidence.
5. Keep book-era, Java-specific advice contextualized instead of promoting it to a
   universal language rule.

## Bibliography

- Robert C. Martin, *Clean Code: A Handbook of Agile Software Craftsmanship*,
  Prentice Hall, 2008. [Publisher record](https://www.informit.com/store/clean-code-a-handbook-of-agile-software-craftsmanship-9780132639408).
- Robert C. Martin, "Smells and Heuristics", Chapter 17 of *Clean Code*.
  [Online chapter](https://www.oreilly.com/library/view/clean-code-a/9780136083238/chapter17.xhtml).
- Radu Marinescu, "Detection Strategies: Metrics-Based Rules for Detecting
  Design Flaws", ICSM 2004.
  [DOI](https://doi.org/10.1109/ICSM.2004.1357820).
- Adrian Trifu and Radu Marinescu, "Diagnosing Design Problems in Object
  Oriented Systems", WCRE 2005.
  [DOI](https://doi.org/10.1109/WCRE.2005.15).
- Michele Lanza and Radu Marinescu, *Object-Oriented Metrics in Practice: Using
  Software Metrics to Characterize, Evaluate, and Improve the Design of
  Object-Oriented Systems*, Springer, 2006.
  [DOI and publisher record](https://doi.org/10.1007/3-540-39538-5).
- Michele Lanza and Radu Marinescu, "Identity Disharmonies", Chapter 5 of
  *Object-Oriented Metrics in Practice*.
  [Author-hosted chapter PDF](https://scg.unibe.ch/assets/files/47/mgckdeptwnul3k6wzkurax86qichae/Lanz06a-OOMIP-Chapter5.pdf).
