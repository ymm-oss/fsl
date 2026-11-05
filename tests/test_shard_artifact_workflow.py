# SPDX-License-Identifier: Apache-2.0

"""Calibrated controls for the parser-backed shard artifact workflow audit."""

from __future__ import annotations

import copy
import importlib.util
import shlex
import sys
from pathlib import Path

import pytest

MODULE_PATH = Path(__file__).resolve().parents[1] / ".github" / "scripts" / "validate-shard-artifact-workflow.py"
SPEC = importlib.util.spec_from_file_location("validate_shard_artifact_workflow", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
validator = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = validator
SPEC.loader.exec_module(validator)


@pytest.fixture()
def workflow() -> dict:
    return copy.deepcopy(validator.load_workflow(validator.WORKFLOW))


def step(document: dict, job: str, name: str) -> dict:
    return next(item for item in document["jobs"][job]["steps"] if item.get("name") == name)


def assert_rejected(document: dict, diagnostic: str) -> None:
    errors = validator.audit_workflow(document)
    assert errors, "the isolated mutation must not pass silently"
    assert any(diagnostic in error for error in errors), f"produced={errors!r}; expected={diagnostic!r}"


def test_live_workflow_is_accepted() -> None:
    errors = validator.audit_workflow(validator.load_workflow(validator.WORKFLOW))
    assert errors == []


@pytest.mark.parametrize(
    ("job", "name", "diagnostic"),
    [
        ("rust-tests", "Preserve test-shard inventory", "rust-tests upload name"),
        ("semantic-mutation-operators", "Preserve operator shard evidence", "semantic-mutation-operators upload name"),
    ],
)
def test_one_sided_run_attempt_reintroduction_is_rejected(
    workflow: dict, job: str, name: str, diagnostic: str
) -> None:
    upload = step(workflow, job, name)
    upload["with"]["name"] += "-${{ github.run_attempt }}"
    assert_rejected(workflow, diagnostic)


def test_download_pattern_run_attempt_reintroduction_is_rejected(workflow: dict) -> None:
    download = step(workflow, "rust-workspace", "Download rust test-shard inventories")
    download["with"]["pattern"] += "-${{ github.run_attempt }}"
    assert_rejected(workflow, "rust-workspace download pattern")


def test_overwrite_removal_is_rejected(workflow: dict) -> None:
    upload = step(workflow, "semantic-mutation-operators", "Preserve operator shard evidence")
    upload["with"]["overwrite"] = False
    assert_rejected(workflow, "upload overwrite: expected True, actual False")


def test_provenance_step_removal_is_rejected(workflow: dict) -> None:
    steps = workflow["jobs"]["rust-tests"]["steps"]
    steps.remove(step(workflow, "rust-tests", "Record test-shard provenance"))
    assert_rejected(workflow, "expected exactly one provenance step")


def test_common_helper_bypass_is_rejected(workflow: dict) -> None:
    verify = step(workflow, "semantic-mutation", "Verify operator shard completeness")
    verify["run"] = "./tools/check-shard-union.sh full.txt shard-*.txt"
    assert_rejected(workflow, "semantic-mutation verifier")


def test_aggregator_always_guard_removal_is_rejected(workflow: dict) -> None:
    workflow["jobs"]["rust-workspace"]["if"] = "success()"
    assert_rejected(workflow, "jobs.rust-workspace.if")


def test_dependency_result_guard_removal_is_rejected(workflow: dict) -> None:
    guard = step(workflow, "semantic-mutation", "Require complete operator-shard and mutants-lane evidence")
    del guard["env"]["SEMANTIC_MUTATION_OPERATORS"]
    assert_rejected(workflow, "dependency guard env: expected 'SEMANTIC_MUTATION_OPERATORS'")


@pytest.mark.parametrize(
    ("job", "step_name", "mutated_name", "diagnostic"),
    [
        (
            "semantic-mutation-mutants",
            "Preserve mutation evidence",
            "semantic-mutation-mutants-${{ github.run_id }}-${{ github.run_attempt }}",
            "semantic-mutation-mutants upload name",
        ),
        (
            "fsl-logic",
            "Preserve FSL Logic evidence",
            "fsl-logic-${{ github.run_id }}",
            "fsl-logic upload name",
        ),
    ],
)
def test_unsharded_artifact_attempt_scope_is_preserved(
    workflow: dict, job: str, step_name: str, mutated_name: str, diagnostic: str
) -> None:
    upload = step(workflow, job, step_name)
    upload["with"]["name"] = mutated_name
    assert_rejected(workflow, diagnostic)


@pytest.mark.parametrize("fault", ["drop-shard", "duplicate-shard", "bypass-union", "skip-union", "skip-green", "omit-needs", "unsharded-command", "broaden-tier", "fake-success", "skip-guard"])
def test_generic_mutant_workflow_controls(workflow: dict, fault: str) -> None:
    job = workflow['jobs']['semantic-mutation-mutants']
    if fault in ('drop-shard', 'duplicate-shard'):
        job['strategy']['matrix']['shard'] = [1, 2] if fault == 'drop-shard' else [1, 2, 2]
        diagnostic = 'mutants matrix'
    elif fault in ('bypass-union', 'skip-union'):
        verify = step(workflow, 'semantic-mutation', 'Verify mutant shard completeness')
        verify['run' if fault == 'bypass-union' else 'if'] = 'true'
        diagnostic = 'mutants verifier' if fault == 'bypass-union' else 'mutants verify if'
    elif fault == 'skip-green':
        guard = step(workflow, 'semantic-mutation', 'Require complete operator-shard and mutants-lane evidence')
        guard['run'] = guard['run'].replace('!= "success"', '== "failure"')
        diagnostic = 'all-results guard'
    elif fault == 'omit-needs':
        workflow['jobs']['semantic-mutation']['needs'].remove('semantic-mutation-mutants')
        diagnostic = 'semantic-mutation needs'
    elif fault == 'broaden-tier':
        step(workflow, 'semantic-mutation-mutants', 'Calibrate complete accepted implementation scope')['if'] = "steps.scope.outputs.run == 'true'"
        diagnostic = 'mutants complete tier'
    elif fault in ('fake-success', 'skip-guard'):
        guard = step(workflow, 'semantic-mutation', 'Require complete operator-shard and mutants-lane evidence')
        if fault == 'fake-success':
            guard['env']['SEMANTIC_MUTATION_MUTANTS'] = 'success'
        else:
            guard['if'] = 'false'
        diagnostic = 'guard results' if fault == 'fake-success' else 'guard if'
    else:
        step(workflow, 'semantic-mutation-mutants', 'Calibrate changed implementation semantics')['run'] = './tools/run-semantic-mutation-gate.sh changed --lane mutants'
        diagnostic = 'mutants changed command'
    assert_rejected(workflow, diagnostic)


MUTANT_SPEC = importlib.util.spec_from_file_location('mutant_cohort', MODULE_PATH.parents[2] / 'tools/check-mutant-shard-cohort.py')
assert MUTANT_SPEC is not None and MUTANT_SPEC.loader is not None
mutants = importlib.util.module_from_spec(MUTANT_SPEC)
MUTANT_SPEC.loader.exec_module(mutants)


def detector_build_rows(first_party_fresh: bool = False, third_party_fresh: bool = True,
                        omit: str | None = None) -> str:
    import json
    rows = [dict(reason='compiler-artifact', package_id=f'path+file:///w/{name}#0.1.0', target=dict(name=name), fresh=first_party_fresh)
            for name in sorted(mutants.DETECTOR_FIRST_PARTY_TARGETS) if name != omit]
    rows.append(dict(reason='compiler-artifact', package_id='registry+https://index/#z3-sys@0.1.0', target=dict(name='z3_sys'), fresh=third_party_fresh))
    rows.append(dict(reason='build-finished', success=True))
    return ''.join(json.dumps(row) + '\n' for row in rows)


def mutant_fixture(root: Path, ids: list[str], attempts: tuple[int, int, int] = (1, 2, 1)) -> Path:
    """Synthetic protocol evidence, NOT a claim of actual detector execution."""
    import json
    equivalents = root / 'equivalents.json'
    equivalents.write_text(json.dumps({'entries': []}))
    for index in range(1, 4):
        artifact = root / f'semantic-mutation-mutants-{index}-77'
        artifact.mkdir()
        selected = ids[index - 1::3]
        for name, values in [('global-mutants.json', ids), ('selected-mutants.json', selected)]:
            (artifact / name).write_text(json.dumps([{'name': x} for x in values]))
        report = dict(base_revision='rev', complete=True, diff_scope={'mode': 'changed'},
                      runner={'name': 'cargo-mutants', 'version': '27.1.0'}, configuration='rust/.cargo/mutants.toml',
                      mutants=[dict(id=x, classification='unbuildable') for x in selected])
        (artifact / 'implementation-mutation-report.v1.json').write_text(json.dumps(report))
        baseline = ''.join(f'running {count} tests\ntest result: ok. {count} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;\n'
                           for count in (19, 16))
        if selected:
            raw = artifact / 'mutants.out'
            (raw / 'log').mkdir(parents=True)
            (raw / 'mutants.json').write_text((artifact / 'selected-mutants.json').read_text())
            rows = [dict(scenario='Baseline', summary='Success', phase_results=[dict(phase='Test', process_status='Success', argv=['cargo', 'test', '--test', 'solver_fail_closed', '--test', 'triangulated_assurance'])])]
            rows += [dict(scenario={'Mutant': {'name': x}}, summary='Unviable') for x in selected]
            (raw / 'outcomes.json').write_text(json.dumps(dict(outcomes=rows, cargo_mutants_version='27.1.0', end_time='done', total_mutants=len(selected), caught=0, missed=0, timeout=0, unviable=len(selected), success=0)))
            (raw / 'log/baseline.log').write_text(baseline)
        else:
            (artifact / 'baseline.log').write_text(baseline)
            (artifact / 'detector-build.jsonl').write_text(detector_build_rows())
        mutants.record(artifact, index, 3, 'changed', equivalents, '77', attempts[index - 1], 'rev')
    return equivalents


@pytest.mark.parametrize('ids', [[], ['a'], ['a', 'b', 'c', 'd']])
@pytest.mark.parametrize('attempts', [(1, 2, 1), (2, 2, 2)])
def test_mutant_empty_and_partial_rerun_cohorts(tmp_path: Path, ids: list[str], attempts: tuple[int, int, int]) -> None:
    equivalents = mutant_fixture(tmp_path, ids, attempts)
    mutants.cohort(tmp_path, '77', 2, 'rev', 3, equivalents)


@pytest.mark.parametrize(('fault', 'expected'), [
    ('drop-mutant', 'executed outcome roster'), ('duplicate-mutant', 'executed outcome roster'),
    ('drop-shard', 'missing/extra shard'), ('duplicate-shard', 'duplicate shard'),
    ('wrong-revision', 'wrong run/revision'), ('wrong-run', 'wrong run/revision'),
    ('future-attempt', 'incompatible attempt'), ('zero-selection', 'selected slice'),
    ('baseline-omission', 'baseline omitted'), ('stale-equivalent', 'stale reviewed-equivalent'),
    ('payload-change', 'payload hash'),
    ('wrong-mode', 'report mode'), ('incomplete-report', 'incomplete report'),
    ('unknown-outcome', 'raw/report classification'), ('timeout-outcome', 'raw/report classification'),
    ('raw-drop', 'raw outcome roster'), ('raw-duplicate', 'raw outcome roster'),
    ('baseline-phase-omission', 'baseline test phase'),
    ('empty-baseline-omission', 'baseline omitted'), ('invented-empty-outcomes', 'must not invent'),
    ('empty-first-party-reused', 'stale first-party artifact reused'),
    ('empty-detector-build-omitted', 'detector-build.jsonl'),
    ('empty-detector-run-compiled', 'compiled after the verified build'),
])
def test_mutant_cohort_rejecting_controls(tmp_path: Path, fault: str, expected: str) -> None:
    import json
    import shutil
    equivalents = mutant_fixture(tmp_path, ['a'] if fault.startswith('empty-') or fault == 'invented-empty-outcomes' else ['a', 'b', 'c', 'd'])
    baseline = {str(p.relative_to(tmp_path)): p.read_bytes() for p in tmp_path.rglob('*') if p.is_file()}
    a = tmp_path / 'semantic-mutation-mutants-1-77'
    path = a / 'artifact-provenance.v1.json'
    data = json.loads(path.read_text())
    if fault in ('drop-mutant', 'duplicate-mutant'):
        path = a / 'implementation-mutation-report.v1.json'
        data = json.loads(path.read_text())
        data['mutants'] = data['mutants'][:1] if fault == 'drop-mutant' else data['mutants'] + data['mutants'][:1]
    elif fault == 'drop-shard':
        shutil.rmtree(a)
    elif fault == 'duplicate-shard':
        a = tmp_path / 'semantic-mutation-mutants-2-77'
        path = a / 'artifact-provenance.v1.json'
        data = json.loads(path.read_text())
        data['shard']['index'] = 1
    elif fault == 'wrong-revision':
        data['head_revision'] = 'foreign'
    elif fault == 'wrong-run':
        data['run_id'] = '88'
    elif fault == 'future-attempt':
        data['run_attempt'] = 3
    elif fault == 'zero-selection':
        path, data = a / 'selected-mutants.json', []
    elif fault == 'baseline-omission':
        (a / 'mutants.out/log/baseline.log').write_text('running 19 tests\n')
    elif fault == 'empty-baseline-omission':
        (tmp_path / 'semantic-mutation-mutants-2-77/baseline.log').write_text('running 19 tests\n')
    elif fault == 'invented-empty-outcomes':
        (tmp_path / 'semantic-mutation-mutants-2-77/mutants.out').mkdir()
    elif fault == 'empty-first-party-reused':
        (tmp_path / 'semantic-mutation-mutants-2-77/detector-build.jsonl').write_text(detector_build_rows(first_party_fresh=True))
    elif fault == 'empty-detector-build-omitted':
        (tmp_path / 'semantic-mutation-mutants-2-77/detector-build.jsonl').unlink()
    elif fault == 'empty-detector-run-compiled':
        log = tmp_path / 'semantic-mutation-mutants-2-77/baseline.log'
        log.write_text('\x1b[1m\x1b[92m   Compiling\x1b[0m fsl-core v1 (/w)\n' + log.read_text())
    elif fault == 'wrong-mode':
        data['mode'] = 'complete'
        path.write_text(json.dumps(data))
    elif fault == 'incomplete-report':
        path = a / 'implementation-mutation-report.v1.json'
        obj = json.loads(path.read_text())
        obj['complete'] = False
        path.write_text(json.dumps(obj))
    elif fault in ('unknown-outcome', 'timeout-outcome', 'raw-drop', 'raw-duplicate', 'baseline-phase-omission'):
        path = a / 'mutants.out/outcomes.json'
        obj = json.loads(path.read_text())
        if fault in ('unknown-outcome', 'timeout-outcome'):
            obj['outcomes'][1]['summary'] = 'Unknown' if fault == 'unknown-outcome' else 'Timeout'
        elif fault == 'raw-drop':
            obj['outcomes'].pop()
        elif fault == 'raw-duplicate':
            obj['outcomes'].append(obj['outcomes'][1])
        else:
            obj['outcomes'][0]['phase_results'] = []
        path.write_text(json.dumps(obj))
    elif fault == 'stale-equivalent':
        for artifact in tmp_path.glob('semantic-*'):
            p = artifact / 'artifact-provenance.v1.json'
            obj = json.loads(p.read_text())
            obj['mode'] = 'complete'
            p.write_text(json.dumps(obj))
        equivalents.write_text(json.dumps({'entries': [{'mutant_id': 'stale'}]}))
    else:
        path = a / 'mutants.out/log/baseline.log'
        path.write_text(path.read_text() + 'changed\n')
    if fault in ('drop-mutant', 'duplicate-mutant', 'duplicate-shard', 'wrong-revision', 'wrong-run', 'future-attempt', 'zero-selection'):
        path.write_text(json.dumps(data))
    with pytest.raises((ValueError, OSError), match=expected) as rejected:
        mutants.cohort(tmp_path, '77', 2, 'rev', 3, equivalents)
    print(f'detector {fault}: produced={str(rejected.value)!r}; expected_contains={expected!r}')
    # Exact byte restoration, then healthy twice; isolated fixture faults never
    # modify the live workflow, sources, detector tests, or runner outcomes.
    for path in tmp_path.iterdir():
        if path.is_dir():
            shutil.rmtree(path)
        else:
            path.unlink()
    for name, content in baseline.items():
        path = tmp_path / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
    assert {str(p.relative_to(tmp_path)): p.read_bytes() for p in tmp_path.rglob('*') if p.is_file()} == baseline
    for _ in range(2):
        mutants.cohort(tmp_path, '77', 2, 'rev', 3, equivalents)


def test_unsharded_complete_rejects_listed_equivalent_missing_from_outcomes(tmp_path: Path) -> None:
    import json
    import os
    import subprocess

    # Fixture-only equivalence: never add an exemption to the repository manifest.
    equivalents = mutant_fixture(tmp_path, ['reviewed-fixture-id'])
    equivalents.write_text(json.dumps({'entries': [{'mutant_id': 'reviewed-fixture-id', 'rationale': 'fixture only'}]}))
    output = tmp_path / 'semantic-mutation-mutants-1-77'
    (output / 'artifact-provenance.v1.json').unlink()
    report_path = output / 'implementation-mutation-report.v1.json'
    report = json.loads(report_path.read_text())
    report['diff_scope']['mode'] = 'complete'
    report_path.write_text(json.dumps(report))
    gate = (MODULE_PATH.parents[2] / 'tools/run-semantic-mutation-gate.sh').read_text()
    start = gate.index('record_shard() {')
    function = gate[start:gate.index('\n}\n', start) + 3]
    environment = dict(os.environ, output=str(output), shard_spec='', shard_index='1', shard_total='1',
                       mode='complete', equivalents=str(equivalents), root=str(tmp_path),
                       cohort_helper=str(MODULE_PATH.parents[2] / 'tools/check-mutant-shard-cohort.py'))
    # Invoke the actual gate completion function; only the fixture revision is supplied.
    command = ['bash', '-c', function + '\ngit() { printf "rev\\n"; }\nrecord_shard']
    def run():
        return subprocess.run(command, env=environment, text=True, capture_output=True)
    for _ in range(2):
        healthy = run()
        assert healthy.returncode == 0, healthy.stderr
    before = {p: p.read_bytes() for p in (report_path, output / 'mutants.out/outcomes.json')}
    raw = json.loads(before[output / 'mutants.out/outcomes.json'])
    raw['outcomes'] = [r for r in raw['outcomes'] if r['scenario'] == 'Baseline']
    raw['total_mutants'] = raw['unviable'] = 0  # Internally consistent totals, still a dropped ID.
    (output / 'mutants.out/outcomes.json').write_text(json.dumps(raw))
    report['mutants'] = []
    report_path.write_text(json.dumps(report))
    assert mutants.names(output / 'global-mutants.json') == ['reviewed-fixture-id']
    rejected = run()
    assert rejected.returncode != 0, 'unsharded complete accepted a globally listed equivalent absent from actual outcomes'
    assert 'executed outcome roster mismatch' in rejected.stderr, rejected.stderr
    print(f'detector unsharded-listed-equivalent-omitted: produced={rejected.stderr.strip()!r}; expected=executed outcome roster mismatch')
    for path, content in before.items():
        path.write_bytes(content)
    assert all(path.read_bytes() == content for path, content in before.items())
    for _ in range(2):
        healthy = run()
        assert healthy.returncode == 0, healthy.stderr
    assert not (output / 'artifact-provenance.v1.json').exists()


def test_successful_empty_native_listing_is_encoded_but_runner_failure_is_not(tmp_path: Path) -> None:
    import json
    import subprocess

    gate = (MODULE_PATH.parents[2] / 'tools/run-semantic-mutation-gate.sh').read_text()
    start = gate.index('list_inventory() {')
    function = gate[start:gate.index('\n}\n', start) + 3]
    (tmp_path / 'rust').mkdir()
    destination = tmp_path / 'inventory.json'
    def run(body: str, status: int):
        return subprocess.run(['bash', '-c', 'set -e\n' + body + '\nscratch="$1"; args=(); '
                               f'cargo() {{ return {status}; }}\nlist_inventory "$2"',
                               'fixture', str(tmp_path), str(destination)], capture_output=True, text=True)
    for _ in range(2):
        assert run(function, 0).returncode == 0
        assert json.loads(destination.read_text()) == []
    # Calibrate loss of the successful-empty encoding, then restore exact body.
    faulty = function.replace("[ -s \"$destination\" ] || printf '[]\\n' >\"$destination\"", ':')
    assert faulty != function
    assert run(faulty, 0).returncode == 0
    with pytest.raises(json.JSONDecodeError):
        json.loads(destination.read_text())
    for _ in range(2):
        assert run(function, 0).returncode == 0
        assert json.loads(destination.read_text()) == []
    failed = run(function, 42)
    assert failed.returncode == 42 and destination.read_bytes() == b''


def seed_source(root: Path, *units: tuple[str, str, str]) -> Path:
    """A Cargo-shaped debug directory; every unit is (package, crate, hash)."""
    source = root / 'source'
    for package, crate, unit_hash in units:
        for kind in ('.fingerprint', 'build'):
            (source / kind / f'{package}-{unit_hash}').mkdir(parents=True)
            (source / kind / f'{package}-{unit_hash}' / 'marker').write_text(package)
        (source / 'deps').mkdir(exist_ok=True)
        (source / 'deps' / f'lib{crate}-{unit_hash}.rlib').write_text(package)
        (source / 'deps' / f'{crate}-{unit_hash}.d').write_text(package)
    return source


def seed_metadata(root: Path) -> Path:
    import json
    path = root / 'metadata.json'
    path.write_text(json.dumps({'packages': [
        {'name': 'z3-sys', 'source': 'registry+https://index'}, {'name': 'fsl-core', 'source': None},
        {'name': 'both', 'source': 'registry+https://index'}, {'name': 'both', 'source': None}]}))
    return path


Z3_UNIT = ('z3-sys', 'z3_sys', 'a' * 16)
FIRST_PARTY_UNIT = ('fsl-core', 'fsl_core', 'b' * 16)


def test_seed_copies_third_party_units_and_their_mtimes_only(tmp_path: Path) -> None:
    import os
    source = seed_source(tmp_path, Z3_UNIT, FIRST_PARTY_UNIT, ('both', 'both', 'c' * 16), ('unknown', 'unknown', 'd' * 16))
    old = source / '.fingerprint' / f'z3-sys-{"a" * 16}' / 'marker'
    os.utime(old, ns=(1_000_000_123, 1_000_000_123))
    target = tmp_path / 'target' / 'debug'
    mutants.seed(source, target, seed_metadata(tmp_path))
    copied = sorted(str(p.relative_to(target)) for p in target.rglob('*') if p.is_file())
    assert copied == sorted([f'.fingerprint/z3-sys-{"a" * 16}/marker', f'build/z3-sys-{"a" * 16}/marker',
                             f'deps/libz3_sys-{"a" * 16}.rlib', f'deps/z3_sys-{"a" * 16}.d'])
    assert (target / '.fingerprint' / f'z3-sys-{"a" * 16}' / 'marker').stat().st_mtime_ns == 1_000_000_123


@pytest.mark.parametrize(('fault', 'expected'), [
    ('missing-source', 'No such file'), ('no-third-party-unit', 'no third-party unit'),
    ('shared-hash', 'hash shared'), ('no-first-party-metadata', 'no first-party package'),
])
def test_seed_rejecting_controls(tmp_path: Path, fault: str, expected: str) -> None:
    import json
    units = {'no-third-party-unit': [FIRST_PARTY_UNIT],
             'shared-hash': [Z3_UNIT, ('fsl-core', 'fsl_core', 'a' * 16)]}.get(fault, [Z3_UNIT])
    source = tmp_path / 'absent' if fault == 'missing-source' else seed_source(tmp_path, *units)
    metadata = seed_metadata(tmp_path)
    if fault == 'no-first-party-metadata':
        metadata.write_text(json.dumps({'packages': [{'name': 'z3-sys', 'source': 'registry+https://index'}]}))
    with pytest.raises((ValueError, OSError), match=expected) as rejected:
        mutants.seed(source, tmp_path / 'target' / 'debug', metadata)
    print(f'detector {fault}: produced={str(rejected.value)!r}; expected_contains={expected!r}')


@pytest.mark.parametrize(('rows', 'path', 'expected'), [
    (dict(first_party_fresh=True), 'seeded', 'stale first-party artifact reused'),
    (dict(omit='solver_fail_closed'), 'seeded', 'omitted first-party targets'),
    (dict(omit='fsl_verifier'), 'full-fresh', 'omitted first-party targets'),
    (dict(third_party_fresh=True), 'full-fresh', 'reused third-party'),
    (dict(third_party_fresh=False), 'unknown', 'unknown detector target path'),
])
def test_detector_build_rejecting_controls(tmp_path: Path, rows: dict, path: str, expected: str) -> None:
    (tmp_path / 'detector-build.jsonl').write_text(detector_build_rows(**rows))
    with pytest.raises(ValueError, match=expected) as rejected:
        mutants.check_detector_build(tmp_path, path)
    print(f'detector {rows}/{path}: produced={str(rejected.value)!r}; expected_contains={expected!r}')


@pytest.mark.parametrize(('rows', 'path', 'logged'), [
    (dict(), 'seeded', 'path=seeded first_party_rebuilt=4 first_party_reused=0 third_party_reused=1 third_party_rebuilt=0 z3_sys=reused'),
    (dict(third_party_fresh=False), 'seeded', 'path=seeded first_party_rebuilt=4 first_party_reused=0 third_party_reused=0 third_party_rebuilt=1 z3_sys=rebuilt'),
    (dict(third_party_fresh=False), 'full-fresh', 'path=full-fresh first_party_rebuilt=4 first_party_reused=0 third_party_reused=0 third_party_rebuilt=1 z3_sys=rebuilt'),
])
def test_detector_build_logs_the_path_taken(tmp_path: Path, capsys: pytest.CaptureFixture[str], rows: dict, path: str, logged: str) -> None:
    (tmp_path / 'detector-build.jsonl').write_text(detector_build_rows(**rows))
    mutants.check_detector_build(tmp_path, path)
    assert logged in capsys.readouterr().out


def run_prepare_detector_target(root: Path, function: str, lane: str, source: Path, cargo_status: int = 0):
    import shutil
    import subprocess

    shutil.rmtree(root / 'target', ignore_errors=True)
    (root / 'target').mkdir()
    (root / 'scratch/rust').mkdir(parents=True, exist_ok=True)
    (root / 'output').mkdir(exist_ok=True)
    (root / 'repo/rust/target').mkdir(parents=True, exist_ok=True)
    debug = root / 'repo/rust/target/debug'
    if debug.is_symlink():
        debug.unlink()
    debug.symlink_to(source)
    helper = MODULE_PATH.parents[2] / 'tools/check-mutant-shard-cohort.py'
    metadata = (root / 'metadata.json').read_text()
    body = (f'set -euo pipefail\n{function}\ncargo() {{ printf %s {shlex.quote(metadata)}; return {cargo_status}; }}\n'
            f'lane={lane!r}; root={shlex.quote(str(root / "repo"))}; scratch={shlex.quote(str(root / "scratch"))}; '
            f'output={shlex.quote(str(root / "output"))}; cohort_helper={shlex.quote(str(helper))}; '
            f'export CARGO_TARGET_DIR={shlex.quote(str(root / "target"))}\n'
            'prepare_detector_target\nprintf "target_path=%s\\n" "$target_path"\n')
    return subprocess.run(['bash', '-c', body], capture_output=True, text=True)


def gate_function(name: str) -> str:
    gate = (MODULE_PATH.parents[2] / 'tools/run-semantic-mutation-gate.sh').read_text()
    start = gate.index(f'{name}() {{')
    return gate[start:gate.index('\n}\n', start) + 3]


def test_prepare_detector_target_paths_are_logged_and_a_bad_seed_is_discarded(tmp_path: Path) -> None:
    import shutil

    function = gate_function('prepare_detector_target')
    seed_metadata(tmp_path)
    source = seed_source(tmp_path, Z3_UNIT, FIRST_PARTY_UNIT)
    # Healthy, twice: third-party unit present, first-party unit absent.
    for _ in range(2):
        seeded = run_prepare_detector_target(tmp_path, function, 'mutants', source)
        assert seeded.returncode == 0, seeded.stderr
        assert 'detector-target path=seeded reason=third-party-units-copied' in seeded.stdout
        assert 'target_path=seeded' in seeded.stdout
        fingerprints = sorted(p.name for p in (tmp_path / 'target/debug/.fingerprint').iterdir())
        assert fingerprints == [f'z3-sys-{"a" * 16}']
    # Outside the lane the run never reads the seed source.
    unsharded = run_prepare_detector_target(tmp_path, function, '', source)
    assert 'detector-target path=full-fresh reason=not-lane-run' in unsharded.stdout
    assert list((tmp_path / 'target').iterdir()) == []
    # Metadata failure takes the full fresh build.
    no_metadata = run_prepare_detector_target(tmp_path, function, 'mutants', source, cargo_status=7)
    assert no_metadata.returncode == 0 and 'path=full-fresh reason=seed-unusable' in no_metadata.stdout
    assert list((tmp_path / 'target').iterdir()) == []
    # A copy that dies part-way (no deps directory) must leave no partial seed behind.
    shutil.rmtree(source / 'deps')
    partial = run_prepare_detector_target(tmp_path, function, 'mutants', source)
    assert partial.returncode == 0, partial.stderr
    assert 'detector-target path=full-fresh reason=seed-unusable' in partial.stdout
    assert 'target_path=full-fresh' in partial.stdout
    assert list((tmp_path / 'target').iterdir()) == []
    # Calibrate that last assertion: without the discard the partial copy survives.
    faulty = function.replace('rm -rf "$CARGO_TARGET_DIR"', ':')
    assert faulty != function
    kept = run_prepare_detector_target(tmp_path, faulty, 'mutants', source)
    assert 'path=full-fresh reason=seed-unusable' in kept.stdout
    assert (tmp_path / 'target/debug/.fingerprint').is_dir()


def test_only_the_zero_mutant_branch_seeds_and_it_verifies_before_the_detectors() -> None:
    gate = (MODULE_PATH.parents[2] / 'tools/run-semantic-mutation-gate.sh').read_text()
    branch = gate.index('if [ "$selected_mutant_count" -eq 0 ]; then')
    runner = gate.index('cargo mutants "${args[@]}"\n)\nrunner_status')
    assert gate.count('\n  prepare_detector_target\n') == 1
    prepare = gate.index('\n  prepare_detector_target\n')
    verify = gate.index('"$cohort_helper" check-detector-build "$output" "$target_path"')
    detectors = gate.index('| tee "$output/baseline.log"')
    assert branch < prepare < verify < detectors < runner
    assert gate.count('"$cohort_helper" seed ') == 1
    # The verified build and the detector run stay one Cargo selection.
    selection = ('cargo test --locked -p fsl-verifier -p fslc-rust \\\n'
                 '      --test solver_fail_closed --test triangulated_assurance')
    assert gate.count(selection) == 2
