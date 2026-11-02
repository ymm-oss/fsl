<!-- SPDX-License-Identifier: Apache-2.0 -->

# Root-authored synchronized arguments (#1517)

Status: **Accepted contract; source implementation reviewed, execution gates open (#1517).**
This amends DESIGN-compose and DESIGN-check-executable-forms. Review of the
78ff +dad source does not establish product, merge or full-v5 completion.

## Admission and context

Check EVERY authored synchronized actual against its rewritten formal type
before formal substitution can erase or duplicate it, INCLUDING unused formals.
Retain the alias-resolved original expression, root action identity, source-order
ordinal, authored argument span and private root-file identity in a core-owned
obligation queue. Type it using the complete model and ROOT HEADER parameter,
state, constant and enum context, never additional-body lets/requires bindings.
Reuse the existing checker and action-parameter machinery, with authored fallback
available from the beginning: no second validator, partial model or synthetic action.
Compatible numeric carriers remain accepted; numeric range mismatch remains a
verification-time type-bound concern, not a new static range gate.

## Ownership and ranges

Add public `SyncRef.arg_spans: Vec<Span>`: each half-open range covers the whole
authored expression (including its own parentheses), from before expression parsing
through its last token, excluding comma/call close. Require
`args.len() == arg_spans.len()` BEFORE zip/collection. Malformed manual ASTs fail
closed; never truncate or default to the action span. This breaks public Rust
SyncRef constructors; inventory and update ALL constructors/tests.

Keep private root identity separate from diagnostic presentation. Clone preserves
obligations; every KernelSpec constructor initializes them; `with_source_file`
stamps/rebinds private identity with existing stamping semantics. Model construction
consumes the queue. ModelError.span uses the whole argument unless a genuine inner
span lies within this SAME actual's authored byte range; an expanded definition or
out-of-argument location falls back to the whole actual. Do not patch unknown 1:1
after checking. Root presentation uses a non-component origin with source_file=None:
bare `… at 5:16`, not `root.fsl:…`, no component wrapper/related component cause.
Existing component wrappers remain unchanged; no synthetic public registry targets.

For the regression root, `true` is 5:16–20 and `part.h[0]` is 5:16–25.
LSP UTF-16 ranges are respectively (4,15)–(4,19) and (4,15)–(4,24);
CLI/Worker loc remains the start 5:16. Original action metadata stays **4:3**.
Manual public Kernel projection keeps its old fields/bytes, not arg metadata;
current compose export refusal for component filenames is not a successful export.

## Error phases

1. Existing lowering parse/I/O/dependency/name/alias/action/arity/rewrite/hygiene failures.
2. Existing declaration/constant/type/parameter assembly failures.
3. Queued actual typing in authored action/reference/argument order, not HashMap order.
4. Existing init/component expression checks.

A later immediate alias/arity failure can therefore beat an earlier Bool actual.
There is no global authored-order promise across phases; retain competing controls.

## Validated versus raw conversion

Add fallible public `KernelSpec::into_validated_syntax(self) -> Result<SurfaceSpec,
ModelError>`: validate an exact clone through the existing FULL build_model, then
return the ORIGINAL surface only on success. `syntax`, `kernel_ast`, `into_syntax`
remain raw, nonvalidating, lossy projections. `build_surface_model` checks ONLY its
supplied SurfaceSpec; it cannot recover erased unused actuals. Successful raw
projection bytes remain unchanged; intentional raw-projection tests remain raw.

Migrate the three production fslc main conversion paths: reachable counterfactual,
invariant counterfactual, and mutate Business/Requirements/Compose, retaining each
existing failure policy without unwrap/panic. The direct-Spec causal-expectation
tool is not compose-queue erasure. Each migrated invocation adds ONE FULL CORE
MODEL VALIDATION, no extra solver. Representative actual-caller cost measurement
is recorded only at whole-CLI scope, not as isolated incremental cost or free work. The seven source owners are syntax surface.rs/parser.rs,
core compose.rs/lib.rs/model.rs/typecheck.rs and fslc/src/main.rs.

## Breaking removals and pending evidence

Two ACTUAL known check-ok forms become check-error under this adopted target:
`part.put(true)` and `part.put(part.h[0])` when Piece.put(a: K) never uses a
(body `last = 0`, K=0..1, h: relation K -> K). Both checked ok twice on own
17eb2b2be8f1cd8f7856ad1ae9d40cabe38e1588 pristine CLI SHA256
`ad05c1349bc794feb4b728d1877cff2991f3906ba76a22449fd8005f06b85bd1`.
Exact Stage A characterization root SHA256 identities:

| Form | Root SHA256 |
|---|---|
| unused Bool | `0e713efcd02ef1e3e3d6a9c3619d8e0ffa7db26196b6b7182b2e5982e12a4907` |
| unused relation index | `73e678576bcc1d25cb475911a1406a08cc8dceaf2344fb9af5b1b5d90eb52038` |

Both use piece SHA256 `063f8279d8588f2f3c3b56d285a578b14504691c021700ec59d1f7a2d40cef70`.
Measured `check` removal union: **11 complete contexts /9 root-component pairs
/22 mode-context pairs**. Base `c16cf35dc424619d1e1f43a340e6cf7cac23ebcb`; candidate
`78ffb38f714c0fd44e63148e20d34f9ebd5c4563` plus seven-file source patch
`dad156bf383f3d8907c5275bcb44cce189c899076123ad7b52d138b6c81461c1`. Each mode is
stable or FSL5-opt-in on unchanged source, twice per side. Every transition below
is check-ok/exit0 → semantics-error/exit2. The nine new exact three-file `check c.fsl`
contexts are preservation only; physical direct.fsl presence grants no direct/API/
verify coverage. This finite measured union is not runtimefull472 or issue acceptance.

| ID | Exact complete-context key | Pair | Changed admission / candidate loc |
|---|---|---|---|
| R01 | `1cae9cf75cb49d332ab850296a1902db6e08971cbae68ab7229309b979632f05` | P01 | unused true + relation index, argument order; 5:16 |
| R02 | `456840abf00faf2844ad3223e80abab2efb03e62ec5c2fc4c6c5cef9daa45af4` | P02 | body-only x after let; 5:16 |
| R03 | `5aa3baefa078f9ddc0a1d5ed5157e9bd6295df700bf86536d7042d774e69e0d2` | P03 | true then later action index; 5:16 |
| R04 | `8d7b0467683a00853cc958fcf91c33e6e23d32c7cd5506d1401029e98338b5d9` | P04 | relation index; no good sibling; 5:16 |
| R05 | `a1760ef370b7c9164f0eda14b36c84dfa208b847c75fb6ecb8bfa8e5faa8df22` | P05 | unused true; no good sibling; 5:16 |
| R06 | `ab059cd5d0c142246d68ccb9ce2ea193e43ce9b4365e6a865e0a7b46c63942ac` | P06 | true then second reference index; 5:16 |
| R07 | `ab291946a038aa0e77692160e7516fa40a75b8781797c928405dddfd8949bbbe` | P04 | relation index + offered good sibling; 5:16 |
| R08 | `acc62ea03d44de4dd0ddbcd65d4f73e4737e914153f50bed7832a4bb4905da3b` | P07 | body-only x after let/requires; 5:16 |
| R09 | `cd076b93cba275c70735ba2a73731c1935fabcdb0fe16f3246b5ba615e720ded` | P08 | parenthesized unused true; 5:16 |
| R10 | `dd449ab57104f7c5a53e33d3f756937344e76161615f960cb821e2e62372adff` | P05 | unused true + offered good sibling; 5:16 |
| R11 | `91412ae303f1f1877fcdcea2e7e35912726e9832662e0c49d971d8ca09a9bc02` | P09 | second ref `(0), (true),`; 6:37 |

All entries are `root.fsl` declaring compose Root, with `piece.fsl` declaring
spec Piece, K=0..1, h:relation K->K, last:K initialized0 and unused put formal(s)
whose body is last=0. P01/P09 have two formals; P06 adds second(a:K) {}; the
others have one formal. Other declaration/spacing differences remain identified
by these exact byte hashes, not normalized away.

| Pair | Exact root SHA256 | Exact piece.fsl SHA256 |
|---|---|---|
| P01 | `e034202ed5365c3076ca51b6e9c8e1513926717b045dc9de47e2953a0343dd3b` | `f5d9050744bb33c195afa70fad64c390583898565507c62144df19cf750afa86` |
| P02 | `5c6c7f1391941f97e0af91b5c1ca01ea6fe4edc792bb2ea1009789c6a3516f93` | `063f8279d8588f2f3c3b56d285a578b14504691c021700ec59d1f7a2d40cef70` |
| P03 | `e61b94ebe2f2aba93df7b464afb94cfe756e353527054721798c6f7c49417d35` | `063f8279d8588f2f3c3b56d285a578b14504691c021700ec59d1f7a2d40cef70` |
| P04 | `73e678576bcc1d25cb475911a1406a08cc8dceaf2344fb9af5b1b5d90eb52038` | `063f8279d8588f2f3c3b56d285a578b14504691c021700ec59d1f7a2d40cef70` |
| P05 | `0e713efcd02ef1e3e3d6a9c3619d8e0ffa7db26196b6b7182b2e5982e12a4907` | `063f8279d8588f2f3c3b56d285a578b14504691c021700ec59d1f7a2d40cef70` |
| P06 | `64fefc9802089548b6fa2743cf3172264585ac869f3914e1fb1cd4834326ec53` | `7dcc8919022832f96b50dfbcd7c7b5c6b009dea6fb7e8740f360e2cfa672af75` |
| P07 | `5e92fd04a07d15a99a0612920911153ede546ed2c2e95dd3030c4d01e094271b` | `063f8279d8588f2f3c3b56d285a578b14504691c021700ec59d1f7a2d40cef70` |
| P08 | `40b3935dec0b76b3d5e6569b76cd98559b190939be45adae98764da57e5328af` | `063f8279d8588f2f3c3b56d285a578b14504691c021700ec59d1f7a2d40cef70` |
| P09 | `fb95cf70d9a759ada865a3d83260f867dbfea4634ef12129c7699399eaff8fd9` | `f5d9050744bb33c195afa70fad64c390583898565507c62144df19cf750afa86` |

The exact good.fsl SHA is `7a79980377068e1567fd3394a3f214323e76297897906dba63a7cbcfa532889c`,
declaring spec Good with seen:Bool initializedfalse and idle() {}. It is offered
but unreferenced in R01/R02/R03/R06/R07/R08/R09/R10; R11 references it as first
and calls first.idle() before part.put. R04/R05 contain only root.fsl+piece.fsl.
All other contexts retain all three files. Read roles and presence are distinct.

The mandatory610 on-disk forms had zero removals; inline sources are not an empty set.
Retained disk-sweep invocation (historical execution, not a new run):
```sh
/usr/bin/lockf -k "/Users/rizumita/.cache/fsl-heavy.lock" \
  "/Users/rizumita/workspace/fsl-worktrees/v5-1517-root-source/scripts/cx-log" \
  1517-source-78ff-U7-corpus610-frozen47-2x -- \
  /Applications/Xcode-26.6.0.app/Contents/Developer/usr/bin/python3 \
  "/Users/rizumita/workspace/fsl-worktrees/v5-1517-root-source/.claude/work/1517-source-corpus.py"
```
It ran from the named candidate worktree with `FSLC_CACHE=off`.
The script unsets `FSLC_UNSTABLE` in stable mode and sets it to `fsl5` only
for FSL5-opt-in, without changing any source header. In each mode it invokes
each own-tree binary as `fslc check <original roster path>` twice, with each
binary's own materialized tree as cwd. The recorded wrapper uses Rust1.98,
`CARGO_NET_OFFLINE=true`, jobs4/threads4, and own H target/TMP. The base/head
SHAs and seven-source candidate patch identity are stated above; form count is610.
R04/R07 andR05/R10 each share root/piece bytes but differ in offered good.fsl, so
all11 full contexts remain. Multiple actual/reference/action order, body-only x,
parentheses and the trailing comma are breaking acceptance removals,
not new permission for body lets to bind originals. Historical Stage A identities
above remain their own observations. Original per-cohort H binaries831ab0e2 and
1caf9f0c are not relabeled as later d98f6ba9.

Implementation evidence must also cover used/unused rejection and valid header/
alias/forward const/state/numeric controls, missing/swapped metadata and queue
omission, Clone/stamping/rebinding, validated conversion accept/reject and all
three callers, phase competition, parentheses/inner/UTF-16 real stdio, complete
native/Worker packets and model/manual-AST/Kernel preservation or exact refusal.
The six direct API controls, including validated/raw conversion controls, compiled
and passed on78ff+dad in the retained workspace run; this is not compile-RED
evidence. The three caller bypasses were measured in seven states
H0/R/RR/I/IR/M/MR with four builds and16CLI observations, two repetitions on fixed
valid inputs. R/I/M honestly survived; RR/IR/MR preserved output after exact source
restoration and reuse of the freshly built H0 binary, without restore builds.
Complete stdout/JSON/exits and all non-time stderr bytes matched; only the observed
mutate elapsed scalar was excluded from literal stderr equality and still required
its exact diagnostic framing. These are stable-input preservation measurements,
not caller-specific semantic kills or isolated incremental validation costs;
earlier-owner rejection is not a late-caller kill. Isolated core faults twice, exact restore twice,
#1254/#1622 preservation, required focused/workspace/product gates and independent
implementation review remain mandatory before issue/product completion.
