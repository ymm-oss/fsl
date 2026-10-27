# Panic-site audit outside fsl-core (#1475)

Status: accepted (#1475; split-outs #1494 and #1495).

## Scope and population

This is a bounded historical audit, not a claim that the whole CLI is panic-free.
The production targets are `fsl-tools` library, `fslc-rust` library, and `fslc`
binary; tests, generated-language strings, dependencies and resource exhaustion
are not silently counted as checked-document defects.

The six Clippy families are `indexing_slicing`, `unwrap_used`, `expect_used`,
`panic`, `unreachable` and `todo`. Enumeration used native Darwin/aarch64,
Rust 1.98.0 with
`cargo clippy --manifest-path rust/Cargo.toml -p fsl-tools -p fslc-rust --lib
--bin fslc --offline --locked --message-format=json -- -A clippy::all
-W clippy::indexing_slicing -W clippy::unwrap_used -W clippy::expect_used
-W clippy::panic -W clippy::unreachable -W clippy::todo`.
Supplementary fold/seed/name-index searches found arithmetic outside those lints.

The initial inventory at `2debc32a67d5e459110a4fd61fce9cbcf93c3fe7`, rechecked
by diagnostic statement at `8e54e862cb809121bcd33de53645795bf94a1df5`, contains
529 diagnostics: tools 192, library 131, binary 206. These are diagnostic counts,
not unique expressions: immutable JSON indexing may produce multiple diagnostics
for one statement. Lint counts are indexing 395, expect 119, unwrap 8,
unreachable 5, panic 2, todo 0. The earlier #1464 population of 510 grew by 19;
all 62 previously classified and 448 previously unaudited diagnostics, plus the
19 additions, are owned below. `db.rs` has no diagnostic in this population;
its #1473 vocabulary change adds no panic construct. The intervening `main.rs`
changes at the recheck are comments, not weakened bounds/producer invariants.

- **S (523):** the diagnostic's panic condition is excluded by the stated local
  bounds, shape, producer or caller invariant. This does not certify every other
  operation in the same function. Immutable `serde_json::Value` indexing returns
  Null on missing/wrong-shaped input, unlike Vec or mutable JSON indexing.
- **P-A (3):** reachable from auxiliary evidence, source-label or manifest input.
  These are fixed through shared boundaries below, not reclassified as S.
- **E (3):** document-independent OS thread-creation failure, retained as an
  explicit exclusion and tracked by #1494, not tested by exhausting the host.
- **Supplemental P-D1:** document-derived time arithmetic, outside the six lints.
- **Supplemental P-A4:** approval timestamp arithmetic, outside the six lints.
- **Supplemental P-A2 sibling:** the fresh-schema debug assertion, also outside
  the six lints. It is part of the same source-label family, not an extra
  diagnostic in the 529-count population.

## Classification ledger

IDs are target-local ordering by file and diagnostic source span in the named
initial inventory, not stable identifiers for future code. T paths are under
`rust/fsl-tools/src/`; L/B paths under `rust/fslc/src/`. Ranges retain every
diagnostic, including nested JSON access on the same statement. The locations
are historical witnesses, not current-head line promises. A future guard,
producer, collection-shape or caller change requires re-review of its reason.

| IDs | Class | File / local reason |
|---|---|---|
| T001-T015 | S | agent.rs:363-405,575-763: insert info before walk; require_child validates endpoints; child paths and infos share the children; high_tools nonempty before [0] |
| T016 | S | ai_stochastic.rs:86-114: empty requirements rejected before first element |
| T017 | S | ai_stochastic.rs:217-220: None case returns before unwrap |
| T018-T020 | S | ai_stochastic.rs:239-240,358: immutable JSON |
| T021 | S | ai_stochastic.rs:236-269,471-476: invalid dataset emits a check; nonempty requirements each emit a check |
| T022-T026 | S | ai_stochastic.rs:555-655: immutable JSON |
| T027-T034 | S | analysis.rs:387,732-776: checked annotations; eligible len bounds; deltas derive from the same weights keys |
| T035-T036 | S | analysis.rs:929,936: immutable JSON |
| T037 | S | analysis_export.rs:120-146: insert IDs for all nodes, then walk those nodes |
| T038-T051 | S | analysis_graph.rs: endpoint closure; SCCs nonempty; path positions recorded before push; checked writer/reader keys; len bounds; generated JSON objects; insert distance before enqueue |
| T052-T056 | S | causal.rs:1187,1277-1283: nonempty ordered claims/windows(2); find_cycle pushes a claim before returning a cycle |
| T057-T062 | S | causal.rs:1438-1447: Tarjan inserts indices/lowlinks before recursive lookup; contains/on_stack guards |
| T063-T076 | S | causal_analysis.rs:114-126,273-291: SCC partition of all variables; resolved endpoints; component vectors/keys from the same enumeration |
| T077-T079 | S | causal_analysis.rs:365-382,388,428,494: reach_any inserted for all and only the same interventions |
| T080-T081 | S | causal_analysis.rs:595,604: classes inserted for every declared feedback; numeric sum is separately P-D1 |
| T082-T088 | S | causal_analysis.rs:680-923: earliest/reach_any maps built from the same interventions |
| T089-T090 | S | causal_evidence.rs:125: len10 guard short-circuits before fixed byte positions |
| T091 | S | causal_ledger_projection.rs:127-132: outer condition excludes Subsumes arm |
| T092-T093 | S | causal_ledger_projection.rs:136-202: generated JSON object before as_object_mut |
| T094 | S | causal_projection.rs:186: feedback classes cover all feedback IDs; numeric sum separately P-D1 |
| T095-T118 | S | db_import.rs:79-140,283-303: SQL/Prisma word/part len guards before every index |
| T119 | P-A | document_check.rs:122-123: newline source label can invalidate freshly rendered frontmatter (P-A2) |
| T120 | P-A | document_check.rs:217-218: JSON evidence.result can inject a reserved marker into fresh rendering (P-A1) |
| T121 | S | document_digest.rs:44-90: serialization of recursive Value, no custom serializer/map-key failure |
| T122 | S | document_glossary.rs:144-164: missing locale becomes Err before expect |
| T123-T124 | S | document_project.rs:376-416: source_file/span Some filter before span expect |
| T125 | S | document_render.rs:1181: immutable JSON |
| T126 | S | document_render.rs:1459-1465: len0/1/2 separate arms; split_last only for len>=3 |
| T127-T132 | S | document_render.rs:1483-1800: immutable JSON |
| T133 | S | document_render_expr.rs:229-235: len0/1/2 separate arms; split_last only for len>=3 |
| T134-T135 | S | document_render_expr.rs:431-468: contains and args.len==1 before args[0] |
| T136 | S | document_views.rs:537-554: requires.len==guards.len+1 before guard enumerate index+1 |
| T137-T138 | S | domain.rs:99-107 generated Object; domain_codegen.rs:986 chars enumerate and index>0 |
| T139-T150 | S | html.rs type_text: CLI public_type produces the fixed array arities; no arbitrary raw JSON type input |
| T151-T154 | S | html.rs:301-331: positions/actions/states share count; position-success indices |
| T155-T156 | S | html.rs:971-986: edge array len2 checked before [0]/[1] |
| T157-T163 | S | ledger.rs:39,186: checked annotations, successful position in same declarations/candidates; other JSON reads immutable |
| T164-T181 | S | mutate.rs:61,240-245,769-960: original AST indices into independent same-length clones; statements nonempty before [0] |
| T182-T183 | S | refinement_analysis.rs:62,68: immutable JSON |
| T184-T188 | S | testgen.rs:221,367: validate same step before object/action/params; common path prefix from zip count |
| T189-T190 | S | testplan.rs:356,380,472-503: find_vector checks outcome.kind and returns its existing outcome |
| T191 | S | testplan.rs:603: CLI passes conformance generated from the same model, whose vectors always contain action/outcome objects |
| T192 | S | undecided.rs:187: checked requirement annotations |
| L001-L025 | S | coverage.rs:150-858: immutable JSON, including Value [0] (not Vec) |
| L026 | S | frontend_output.rs:62-78: empty unparsed list returns before [0] |
| L027 | S | frontend_output.rs:439-458: generated Object before adding suggestion |
| L028-L044 | S | lib.rs:213-291: MT array624, loops/mod624, index reset and key-index wrap; conversions fit supported 32/64-bit platforms |
| L045 | S | lib.rs:371-375: enabled nonempty and below(upper)<upper |
| L046-L051 | S | migration.rs:385-410,635: generated/immutable JSON; edits[0] only in len1 arm |
| L052 | S | migration.rs:679: CLI-owned warnings Array or inserted Array, not arbitrary external JSON |
| L053-L057 | S | migration.rs:796-819: byte scanner boundaries and starts_with guards |
| L058 | S | migration.rs:1170-1179: get(index+1) succeeds before same index |
| L059-L070 | S | migration.rs:1210-1325: same lossless tokens/ranges; successful positions; index-1 guarded; lexer UTF-8 boundaries |
| L071 | S | migration.rs:1471-1486: implements.len==1 before [0] |
| L072-L081 | S | migration.rs:1579-1657: matching/position indices from same tokens/tree; lossless source spans |
| L082-L084 | S | process_form.rs:358,385: immutable JSON |
| L085-L086 | S | process_form.rs:465-483: entity_count keys collected from same processes |
| L087-L091 | S | process_form.rs:635-653: emitted.len==edges.len, same edges enumeration |
| L092-L094 | S | process_form.rs:709-742: Reader slices from same token/lexer boundaries; index-1 guarded |
| L095-L107 | S | process_form.rs:783-919,1044: closing/get/position success; parallel transition ranges/events; Err returns before event lookup; partition_point/checked_sub bound index |
| L108-L124 | S | origin_coverage.rs:52-192: immutable JSON |
| L125 | S | reproducer.rs:68-125: exportability/presence validation before expect |
| L126-L127 | S | source_diagnostic.rs:346-347: saturating_sub and supported-platform u32 to usize conversion |
| L128 | S | verification_output.rs:706: real callers reject empty reasons or pass one-element literal |
| L129 | S | verification_output.rs:1478: checked acceptance trace constructor always provides expectation; forbidden traces separate |
| L130 | S | verification_output.rs:1833-1838: setup bound is len or len.saturating_sub(1) |
| L131 | S | verification_output.rs:3558: typed verification-cost Serialize |
| B001 | S | approval.rs:307-312: typed v2 signature serializer produces Object before mutation |
| B002-B008 | S | approval.rs:780-802: len20 before fixed offsets/slices; timestamp arithmetic separately P-A4 |
| B009 | S | approval.rs:917: generated evaluation Object |
| B010-B021 | S | causal.rs:888-1363: compiled/verdicts same length; validated action/binder; generated observed/artifact/lifecycle objects and Value serialization |
| B022-B038 | S | cli_contract.rs:15-246: embedded contract shapes and exhaustive choices; argv get/bounds; parser Ok choices match metadata |
| B039 | S | cli_output.rs:15: error_output produces Object |
| B040-B042 | S | code_audit.rs:223-228: immutable JSON |
| B043 | E | main.rs:136: initial worker OS spawn failure (#1494) |
| B044-B046 | S | main.rs:157,167,182: Value serialization |
| B047 | S | main.rs:859: generated error Object |
| B048-B051 | S | main.rs:981-1003: embedded help contract; position-success prefix index |
| B052-B053 | S | main.rs:1154,1215: outer and inner match same closed command value |
| B054-B057 | S | main.rs:1235-1291: short-circuit len1 / len2 arm |
| B058-B067 | S | main.rs:1852-2313: document/error helpers produce Object |
| B068 | S | main.rs:2358: validated requirements-document record requires reviewed_digest Some |
| B069-B070 | S | main.rs:2515,2688: generated Object / typed drift reasons Serialize |
| B071 | S | main.rs:3311: combination keys derived from same value_ranges |
| B072 | S | main.rs:3435: immutable JSON |
| B073 | S | main.rs:3495: generated not_evaluated Object |
| B074 | S | main.rs:3546: insert section before setting current |
| B075-B076 | S | main.rs:3707-3712: scheduler returns full prefix; results and steps same length |
| B077 | S | main.rs:3766: immutable JSON |
| B078 | S | main.rs:3829: group records groups.len immediately before push |
| B079 | E | main.rs:3859: step OS spawn failure (#1494) |
| B080 | S | main.rs:3927: same steps enumeration and results length |
| B081 | E | main.rs:3947: worker OS spawn failure (#1494) |
| B082-B087 | S | main.rs:3957-4016: steps-derived channel indices; slots/results same length |
| B088 | P-A | main.rs:4144: refine omitted own-file check after keep-going (P-A3) |
| B089-B091 | S | main.rs:4145-4146: target.file and mapping Some guards |
| B092-B097 | S | main.rs:4317-4546: generated detail Object / immutable tests JSON |
| B098-B101 | S | main.rs:4763-4814: validation maps original events 1:1; same variant; known action validated |
| B102-B103 | S | main.rs:4958-4961: generated error/result Object |
| B104-B106 | S | main.rs:5131-5224: existing action/binder and generated observed Object |
| B107 | S | main.rs:5733: slice end len.saturating_sub(1) |
| B108-B111 | S | main.rs:6045-6112: public Kernel/conformance/testplan successful Object constructors |
| B112 | S | main.rs:6144: owned warnings Array or inserted Array |
| B113-B114 | S | main.rs:6564,6610: is_some_and branch |
| B115 | S | main.rs:7037: chars enumerate and index>0 |
| B116-B123 | S | main.rs:8970-9147: same-length independent AST clones |
| B124-B125 | S | main.rs:9417,9427: loop1..trace.len bounds index/index-1 |
| B126 | S | main.rs:9870: weakening filter admits only handled operators |
| B127-B128 | S | main.rs:10196,10393: checked annotations |
| B129 | S | main.rs:11370: starts occurrence validated before selection; find results are UTF-8 boundaries |
| B130-B131 | S | main.rs:11838,12162: immutable JSON |
| B132-B136 | S | main.rs:13132-13352: generated evaluation/error Object, typed record/reasons serialization |
| B137-B147 | S | main.rs:13879-14014: immutable JSON |
| B148-B149 | S | main.rs:14033: windows(2) pair |
| B150-B153 | S | main.rs:14204-14206: names from groups.keys, len-bounded indices |
| B154-B163 | S | main.rs:14337-14374: base_record creates two actions; only that Object is pushed |
| B164-B165 | S | main.rs:14485,14565: immutable JSON |
| B166 | S | main.rs:14671: generated graph node Object |
| B167-B175 | S | main.rs:14854-15096: generated Object/owned graph clone or immutable JSON |
| B176 | S | main.rs:15165: immutable JSON |
| B177 | S | main.rs:15423: missing code_path rejected earlier |
| B178-B179 | S | main.rs:15655: immutable JSON |
| B180-B183 | S | main.rs:15905-16214: generated public/finding/witness Object |
| B184-B186 | S | main.rs:17025-17027: specs.len=mappings.len+1, parallel pushes |
| B187 | S | main.rs:17280: parsed V1 event.state constructed Some by replay_trace.rs |
| B188-B189 | S | main.rs:17339,17357: check/verify/format helpers produce Object |
| B190 | S | name_display.rs:130-140: nonempty rest loop and continue after prefix removal |
| B191 | S | process.rs:38-44: native Z3 future is synchronous, no await/yield/Pending branch on CLI route |
| B192 | S | property_selection.rs:145-155: empty findings return before [0] |
| B193-B195 | S | spec_loading.rs:265-272,348-351: value key existence; relation pair len2 |
| B196-B197 | S | strict_tags.rs:45,65: checked annotations |
| B198 | S | strict_tags.rs:212: CLI-owned warnings Array or inserted Array |
| B199-B200 | S | verification.rs:364: windows(2) pair |
| B201-B203 | S | verification.rs:421,479: monotone_direction Some requires at least one delta, hence values.len>=2 |
| B204 | S | verification.rs:1449-1457: only Ok proof with Some cti reaches expect |
| B205 | S | verification.rs:1559-1562: append-only entries; index recorded before push; removals affect another Vec |
| B206 | S | verification.rs:2697,2879: model Err/Agent refusal returns error2 before status0 finalizer |

## Reachable families and shared fixes

| Family | Boundary | Required behavior and detectors |
|---|---|---|
| P-D1 sum | causal_projection path min; causal_analysis Dijkstra and feedback min | Shared checked addition returns `causal_invalid_model` with message `causal time range overflow while adding claim '<id>'` at the contributing claim's actual ID span. Fallible consumers include graph/timeline/traceability/review. `causal_numeric_cli` tests 3-edge overflow, a shortcut that bypasses Dijkstra to reach path rendering, feedback without interventions, U-1/U/U+1 and check acceptance. |
| P-D1 comparison | review factors2/4; indicator factors3 and3:2 | All four sites use one shared exact scaled comparison, widening before multiplication; no u128 output. Tests fix mathematical threshold sides and product-overflow classifications; fault-injection checks include the otherwise masked second factor3 occurrence. |
| P-A1/P-A2 | document renderer output and checker fresh parsing | Validate frontmatter/body with the existing parser before returning rendering. Fresh checker failures become CheckError::Marker. Preserve MarkerIssue Display and normal bytes/digests; artifact-only body malformation remains drift/exit1. Shared and CLI document/approval tests cover evidence marker injection and newline labels. No label-encoding redesign. |
| P-A3 | chain step-input resolution | Spec and refine share fallible own-file resolution and the same missing-file IO detail. Keep target/mapping precedence, layer status failed, top-level result error/exit2, cutoff and result ordering. chain_cli covers jobs1/2, cutoff, precedence and normal full output. |
| P-A4 | approval canonical date helper | Test digits before arithmetic (no eager then_some evaluation). Existing canonical UTC refusal and normal record/digest bytes remain unchanged. Approval tests cover every digit slot, valid records, document overlays and calendar preservation. |

Auxiliary-input failures retain their existing command-specific refusal envelopes;
locations are retained only where an actual location exists, never fabricated from
an unrelated FSL source span. No new check-acceptance gate, caller-specific poison
guard or catch_unwind is introduced.

Fresh frontmatter that parses but names an unsupported schema is also rejected
with the existing `CheckError::UnsupportedSchema`, not a debug assertion. This
includes a newline label containing a duplicate schema field: the historical
parser's duplicate-key convention is unchanged, and encoding redesign is not
part of this fix.

## Limits and follow-ups

No lint gate is introduced by #1475. #1495 owns a separately
designed reason-bearing inventory gate; ordinary workspace Clippy policy is
unchanged. This ledger is not a live exclusion/allowlist and does not automatically
detect new sites or stale proofs. OS spawn failures remain E under #1494.

Minimum sums and scaled comparisons must not wrap or silently become unknown.
The existing `saturating_add` maximum-bound convention remains unchanged: for
acyclic totals exceeding u64, it does not implement the unrestricted exact
maximum described by DESIGN-causal. Changing that output policy requires a
separate follow-up (#1506); #1475 neither fixes it nor treats saturated maxima
as independent exactness evidence. Stack/allocator exhaustion, all assertions,
arithmetic outside the audited families and dependency behavior are not certified
by this six-lint inventory.
