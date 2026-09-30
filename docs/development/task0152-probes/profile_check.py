#!/usr/bin/env python3
"""Offline exact-specimen/provenance and fail-closed compatibility check.

No production authority, dispatch, or general Messages schema is implemented.
Default runs regression checks; --accept exits 1 for this blocked candidate.
"""
import copy
import hashlib
import json
import math
from pathlib import Path
import sys

DIRECTORY = Path(__file__).with_name('claude-2.1.283-proposal-api-key-v1')
MISSING_CONTRACTS = ('claude-code-20250219', 'prompt-caching-scope-2026-01-05',
                     'mid-conversation-system-2026-04-07', 'per-turn-control-2026-07-01')


def strict_json(text):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError('duplicate_json_key')
            result[key] = value
        return result

    def invalid(value):
        raise ValueError('nonfinite_json')

    def number(value):
        result = float(value)
        return result if math.isfinite(result) else invalid(value)

    result = json.loads(text, object_pairs_hook=pairs, parse_constant=invalid, parse_float=number)
    if not isinstance(result, dict):
        raise ValueError('json_object_required')
    # JSON can escape a surrogate even in otherwise valid UTF-8 input.
    json.dumps(result, ensure_ascii=False).encode('utf-8', errors='strict')
    return result


def normalize(rows):
    normalized = []
    for row in rows:
        item = dict(row)
        body = item.pop('body_utf8')
        item['body'] = strict_json(body) if body else None
        normalized.append(item)
    return normalized


def inspect(rows, expected):
    try:
        normalized = normalize(rows)
        # Full recursive exact comparison includes values AND JSON types.
        # This is an evidence specimen check, not a proof of provider semantics.
        if json.dumps(normalized, sort_keys=True) != json.dumps(expected, sort_keys=True):
            return ['profile_mismatch']
        post = normalized[1]
        body = post['body']
        if type(body['max_tokens']) is not int or not 1 <= body['max_tokens'] <= 128000:
            return ['invalid_declared_tokens']
        strict_json(body['metadata']['user_id'])
        betas = dict(post['headers'])['anthropic-beta'].split(',')
        if not set(MISSING_CONTRACTS).issubset(betas):
            return ['profile_mismatch']
        return ['local_health_accepted', 'unsupported_beta_contract']
    except (ValueError, KeyError, TypeError, IndexError, UnicodeError):
        return ['invalid_fixture_or_json']


def main():
    manifest = strict_json((DIRECTORY / 'run.json').read_text())
    for name, expected_hash in manifest['artifacts'].items():
        assert hashlib.sha256((DIRECTORY / name).read_bytes()).hexdigest() == expected_hash
    for name, key in (('capture_profile.py', 'harness_sha256'), ('gen_profile.sh', 'generator_sha256')):
        assert hashlib.sha256(DIRECTORY.with_name(name).read_bytes()).hexdigest() == manifest[key]
    raw = (DIRECTORY / 'sanitized-capture.jsonl').read_bytes()
    rows = [strict_json(line) for line in raw.decode('utf-8', errors='strict').splitlines()]
    if '--normalize' in sys.argv:
        print(json.dumps({'profile_id': DIRECTORY.name,
                          'source_sha256': hashlib.sha256(raw).hexdigest(),
                          'requests': normalize(rows)}, indent=2))
        return 0
    derived = strict_json((DIRECTORY / 'normalized.json').read_text())
    assert derived['source_sha256'] == hashlib.sha256(raw).hexdigest()
    assert derived['profile_id'] == manifest['profile_id'] == DIRECTORY.name
    expected = derived['requests']
    actual = inspect(rows, expected)
    assert actual == ['local_health_accepted', 'unsupported_beta_contract']
    reformatted = copy.deepcopy(rows)
    reformatted[1]['body_utf8'] = json.dumps(strict_json(rows[1]['body_utf8']), indent=2)
    assert inspect(reformatted, expected) == actual  # formatting is not semantics
    assert manifest['exit'] == 0 and manifest['marker_present'] and not manifest['failures']
    assert manifest['requests'] == len(rows) == 2 and not manifest['timed_out']
    assert dict(rows[1]['headers'])['content-length'] == str(rows[1]['wire_body_bytes'])
    passed = ['complete_capture_to_normalized_equality', 'run_and_source_hashes',
              'actual_local_health_acceptance', 'actual_messages_fail_closed']
    mutations = {
        'unknown_endpoint': lambda r: r[1].update(target='/v1/unknown'),
        'unsupported_destination': lambda r: r[1].update(target='https://example.invalid/v1/messages'),
        'unknown_header': lambda r: r[1]['headers'].append(['x-new-feature', 'enabled']),
        'unknown_beta': lambda r: r[1]['headers'].__setitem__(12, ['anthropic-beta', 'unknown-beta']),
        'duplicate_header': lambda r: r[1]['headers'].append(['content-type', 'application/json']),
        'unknown_nested_feature': lambda r: mutate_body(r, lambda b: b['thinking'].update(remote=True)),
        'server_remote_tool': lambda r: mutate_body(r, lambda b: b['tools'].append({'type': 'web_fetch_20260209', 'name': 'web_fetch'})),
        'client_declaration_outside_named_profile': lambda r: mutate_body(r, lambda b: b['tools'].append({'name': 'Read', 'input_schema': {'type': 'object'}})),
        'uncorrelated_continuation': lambda r: mutate_body(r, lambda b: b['messages'].append({'role': 'user', 'content': [{'type': 'tool_result', 'tool_use_id': 'missing', 'content': 'public'}]})),
        'changed_model': lambda r: mutate_body(r, lambda b: b.update(model='unknown')),
        'boolean_max_tokens': lambda r: mutate_body(r, lambda b: b.update(max_tokens=True)),
        'removed_beta_contract': lambda r: r[1]['headers'].__setitem__(12, ['anthropic-beta', 'context-1m-2025-08-07']),
    }
    for name, mutate in mutations.items():
        changed = copy.deepcopy(rows)
        mutate(changed)
        assert inspect(changed, expected) == ['profile_mismatch'], name
        passed.append(name)
    for malformed in ('{"a":1,"a":2}', '{"a":{"x":1,"x":2}}', '{"a":NaN}',
                      '{"a":Infinity}', '{"a":1e999}', '[]', '{}{}', '{"a":"\\ud800"}', '{'):
        changed = copy.deepcopy(rows)
        changed[1]['body_utf8'] = malformed
        assert inspect(changed, expected) == ['invalid_fixture_or_json']
    passed.append('nine_strict_json_negatives')
    for token in ('128000.0', '128000e0', '"128000"', '0', '128001'):
        changed = copy.deepcopy(rows)
        changed[1]['body_utf8'] = changed[1]['body_utf8'].replace('"max_tokens":128000', '"max_tokens":' + token)
        assert changed != rows and inspect(changed, expected) == ['profile_mismatch']
    passed.append('five_declared_token_negatives')
    print(json.dumps({'profile_id': DIRECTORY.name, 'actual_sequence_accepted': False,
                      'actual_result': actual, 'missing_contracts': MISSING_CONTRACTS,
                      'checks_passed': passed, 'production_authorization': False}, indent=2))
    return 1 if '--accept' in sys.argv else 0


def mutate_body(rows, change):
    body = strict_json(rows[1]['body_utf8'])
    change(body)
    rows[1]['body_utf8'] = json.dumps(body)


if __name__ == '__main__':
    sys.exit(main())
