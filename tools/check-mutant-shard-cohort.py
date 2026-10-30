#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
"""Exact generic-mutant inventories, including genuine empty logical shards."""

from __future__ import annotations

import hashlib
import json
from pathlib import Path
import sys


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ValueError(message)


def load(path: Path):
    return json.loads(path.read_text())


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def names(path: Path) -> list[str]:
    result = [row['name'] for row in load(path)]
    require(all(isinstance(x, str) and x for x in result), 'invalid mutant ID')
    require(len(result) == len(set(result)), 'duplicate mutant ID')
    return result


def inventories(directory: Path, index: int, total: int, mode: str, equivalents: Path):
    require(1 <= index <= total, 'shard index out of range')
    full = names(directory / 'global-mutants.json')
    selected = names(directory / 'selected-mutants.json')
    require(selected == full[index - 1::total], 'selected slice mismatch')
    if mode == 'complete':
        require(set(row['mutant_id'] for row in load(equivalents)['entries']) <= set(full),
                'stale reviewed-equivalent ID in GLOBAL inventory')
    return full, selected


def evidence(directory: Path, selected: list[str], revision: str):
    report = load(directory / 'implementation-mutation-report.v1.json')
    require(report['base_revision'] == revision, 'report revision mismatch')
    require(report['complete'] is True, 'incomplete report')
    require(report['runner'] == {'name': 'cargo-mutants', 'version': '27.1.0'} and
            report['configuration'] == 'rust/.cargo/mutants.toml', 'wrong runner/configuration')
    ids = [row['id'] for row in report['mutants']]
    require(len(ids) == len(set(ids)) and set(ids) == set(selected), 'executed outcome roster mismatch')
    require(all(row['classification'] in ('killed', 'unbuildable', 'reviewed_equivalent')
                for row in report['mutants']), 'unaccepted outcome')
    require(all(row['primary_failing_test'] for row in report['mutants']
                if row['classification'] == 'killed'), 'missing primary failing test')
    baseline = directory / 'baseline.log'
    if selected:
        raw = directory / 'mutants.out'
        outcomes = load(raw / 'outcomes.json')
        require(outcomes['cargo_mutants_version'] == '27.1.0' and outcomes['end_time'] is not None,
                'incomplete/wrong-version raw outcomes')
        require(outcomes['total_mutants'] == len(selected) == sum(outcomes[key] for key in
                ('caught', 'missed', 'timeout', 'unviable', 'success')), 'incomplete raw outcome totals')
        require(names(raw / 'mutants.json') == selected, 'runner selected roster mismatch')
        rows = [r for r in outcomes['outcomes'] if r['scenario'] != 'Baseline']
        actual = [r['scenario']['Mutant']['name'] for r in rows]
        require(len(actual) == len(set(actual)) and set(actual) == set(selected), 'raw outcome roster mismatch')
        classifications = {r['id']: r['classification'] for r in report['mutants']}
        for row in rows:
            expected = {'CaughtMutant': 'killed', 'Unviable': 'unbuildable',
                        'MissedMutant': 'reviewed_equivalent'}.get(row['summary'])
            require(expected is not None and classifications[row['scenario']['Mutant']['name']] == expected,
                    'raw/report classification mismatch')
        bases = [r for r in outcomes['outcomes'] if r['scenario'] == 'Baseline']
        require(len(bases) == 1 and bases[0]['summary'] == 'Success', 'missing successful baseline')
        require(any(p['phase'] == 'Test' and p['process_status'] == 'Success' and
                    'solver_fail_closed' in p['argv'] and 'triangulated_assurance' in p['argv']
                    for p in bases[0]['phase_results']), 'missing baseline test phase')
        baseline = raw / 'log/baseline.log'
    else:
        require(not (directory / 'mutants.out').exists(), 'empty shard must not invent runner outcomes')
    text = baseline.read_text()
    require(all(f'test result: ok. {count} passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;'
                in text for count in (19, 16)),
            'baseline omitted declared 35 detectors')
    require('test result: FAILED' not in text, 'baseline failed')
    return report


def checked_shard(directory: Path, index: int, total: int, mode: str, equivalents: Path, revision: str):
    full, selected = inventories(directory, index, total, mode, equivalents)
    report = evidence(directory, selected, revision)
    require(report['diff_scope']['mode'] == mode, 'report mode mismatch')
    reviewed = {row['mutant_id']: row['rationale'] for row in load(equivalents)['entries']}
    require(all(row['id'] in reviewed and row['reviewed_rationale'] == reviewed[row['id']]
                for row in report['mutants'] if row['classification'] == 'reviewed_equivalent'),
            'unreviewed survivor')
    return full, selected, report


def payload_files(directory: Path) -> list[str]:
    files = ['global-mutants.json', 'selected-mutants.json', 'implementation-mutation-report.v1.json']
    if (directory / 'baseline.log').is_file():
        files.append('baseline.log')
    return files + [str(p.relative_to(directory)) for p in sorted((directory / 'mutants.out').rglob('*'))
                    if p.is_file()]


def record(directory: Path, index: int, total: int, mode: str, equivalents: Path,
           run: str, attempt: int, revision: str):
    checked_shard(directory, index, total, mode, equivalents, revision)
    # Bind every retained raw log/outcome, not just a self-declared success bit.
    files = payload_files(directory)
    payload = dict(schema='fslc.shard-artifact-provenance.v1', lane='semantic-mutation-mutants',
                   run_id=run, run_attempt=attempt, head_revision=revision,
                   shard=dict(index=index, total=total), mode=mode,
                   full_sha256=digest(directory / files[0]), shard_sha256=digest(directory / files[1]),
                   payload_sha256={name: digest(directory / name) for name in files})
    (directory / 'artifact-provenance.v1.json').write_text(json.dumps(payload, indent=2) + '\n')


def cohort(directory: Path, run: str, attempt: int, revision: str, total: int, equivalents: Path):
    artifacts = sorted(p for p in directory.iterdir() if p.is_dir())
    require(len(artifacts) == total, 'missing/extra shard directory')
    require(len(list(directory.rglob('artifact-provenance.v1.json'))) == total, 'duplicate/missing provenance')
    seen, executed = set(), []
    global_ids = None
    global_hash = None
    mode = None
    for artifact in artifacts:
        p = load(artifact / 'artifact-provenance.v1.json')
        require(p['schema'] == 'fslc.shard-artifact-provenance.v1' and
                p['lane'] == 'semantic-mutation-mutants', 'wrong provenance schema/lane')
        require(p['run_id'] == run and p['head_revision'] == revision, 'wrong run/revision')
        require(type(p['run_attempt']) is int and 1 <= p['run_attempt'] <= attempt, 'incompatible attempt')
        index = p['shard']['index']
        require(type(index) is int and 1 <= index <= total and p['shard']['total'] == total,
                'wrong shard metadata')
        require(index not in seen, 'duplicate shard')
        seen.add(index)
        require(artifact.name == f'semantic-mutation-mutants-{index}-{run}', 'wrong artifact name')
        require(p['mode'] in ('changed', 'complete'), 'wrong mode')
        if mode is None:
            mode = p['mode']
        require(p['mode'] == mode, 'incompatible mode')
        full, _, report = checked_shard(artifact, index, total, mode, equivalents, revision)
        require(set(p['payload_sha256']) == set(payload_files(artifact)), 'payload hash inventory mismatch')
        require(report['diff_scope'] == load(artifacts[0] / 'implementation-mutation-report.v1.json')['diff_scope'],
                'incompatible diff scope')
        for name, expected in p['payload_sha256'].items():
            require(not Path(name).is_absolute() and '..' not in Path(name).parts, 'unsafe payload path')
            require(digest(artifact / name) == expected, 'payload hash mismatch')
        require(p['full_sha256'] == digest(artifact / 'global-mutants.json') and
                p['shard_sha256'] == digest(artifact / 'selected-mutants.json'), 'inventory hash mismatch')
        if global_ids is None:
            global_ids, global_hash = full, p['full_sha256']
        require(full == global_ids and p['full_sha256'] == global_hash, 'incompatible GLOBAL inventory')
        executed += [r['id'] for r in report['mutants']]
    require(len(executed) == len(set(executed)) and set(executed) == set(global_ids), 'executed union mismatch')
    print(f'mutant cohort: PASS shards={total} mutants={len(executed)} exact_disjoint_union=true')


def main() -> None:
    command, directory, *args = sys.argv[1:]
    directory = Path(directory)
    if command == 'prepare':
        index, total, mode, equivalents = args
        inventories(directory, int(index), int(total), mode, Path(equivalents))
    elif command == 'validate':
        index, total, mode, equivalents, revision = args
        checked_shard(directory, int(index), int(total), mode, Path(equivalents), revision)
    elif command == 'record':
        index, total, mode, equivalents, run, attempt, revision = args
        record(directory, int(index), int(total), mode, Path(equivalents), run, int(attempt), revision)
    elif command == 'cohort':
        run, attempt, revision, total, equivalents = args
        cohort(directory, run, int(attempt), revision, int(total), Path(equivalents))
    else:
        raise ValueError('expected prepare, validate, record, or cohort')


if __name__ == '__main__':
    try:
        main()
    except (ValueError, KeyError, TypeError, OSError) as error:
        sys.exit(f'mutant cohort: FAIL: {error}')
