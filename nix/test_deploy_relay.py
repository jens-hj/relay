from contextlib import closing
import importlib.util
import json
import os
from pathlib import Path
import tempfile
import io
import sqlite3
import sys
import unittest
from unittest.mock import patch
import urllib.error

spec = importlib.util.spec_from_file_location('deploy', Path(__file__).with_name('deploy-relay.py'))
d = importlib.util.module_from_spec(spec)
spec.loader.exec_module(d)


class DeploymentTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.root.chmod(0o700)
        self.previous = self.root / 'previous'
        self.candidate = self.root / 'candidate'
        self.previous.mkdir()
        self.candidate.mkdir()
        self.active = self.root / 'active'
        self.active.symlink_to(self.candidate)
        self.obj = object.__new__(d.Deployment)
        self.obj.root = self.root
        self.obj.manifest_digest = 'test-manifest'
        self.obj.status_path = self.root / 'status.json'
        self.obj.m = dict(active_link=str(self.active), candidate_bundle=str(self.candidate),
                          previous_bundle=str(self.previous), revision='new', schema_version=18,
                          public_attempts=2, retry_delay=0, web_root=str(self.root / 'web'))
        self.obj.state = dict(stopped=True, activated=True, local_healthy=True, previous_revision='old')
        for name in ('baseline.json', 'drafts.json'):
            d.atomic_json(self.root / name, [])

    def test_transport_pending_then_resume_without_restart(self):
        with patch.object(self.obj, 'local'), patch.object(self.obj, 'public', side_effect=d.Transport('secret')), patch.object(self.obj, 'run') as run:
            self.assertEqual(self.obj.verify(), 2)
            self.assertEqual(self.obj.state['phase'], 'public-verification-pending')
            self.assertEqual(self.active.resolve(), self.candidate)
            run.assert_not_called()
        with patch.object(self.obj, 'local'), patch.object(self.obj, 'public'), patch.object(self.obj, 'run') as run:
            self.assertEqual(self.obj.verify(), 0)
            self.assertEqual(self.obj.state['phase'], 'deployed')
            self.assertNotIn('category', self.obj.state)
            run.assert_not_called()

    def test_activation_tls_failure_keeps_healthy_candidate(self):
        self.obj.link(self.previous)
        self.obj.state = {}
        self.obj.m.update(database=str(self.root / 'db.sqlite3'), protocol_version=11,
                          server_executable=str(self.root / 'relay-server'))
        Path(self.obj.m['server_executable']).touch()
        web = Path(self.obj.m['web_root'])
        web.mkdir()
        (web / 'app_bg.wasm').write_bytes(b'new')
        (self.candidate / 'relay.service').write_text(
            'Environment=RELAY_WEB_ROOT=' + str(web) + '\nExecStart=' + self.obj.m['server_executable'])
        with patch.object(self.obj, 'schema', return_value=18), patch.object(self.obj, 'ready', return_value={'protocol_version': 11}), patch.object(self.obj, 'get_json', side_effect=[[], {'revision': 'old'}]), patch.object(self.obj, 'local'), patch.object(self.obj, 'run') as run, patch.object(self.obj, 'public', side_effect=d.Transport()):
            self.assertEqual(self.obj.activate(), 2)
        self.assertEqual(self.active.resolve(), self.candidate)
        self.assertTrue((self.root / 'database-backup.sqlite3').is_file())
        self.assertEqual(self.obj.state['phase'], 'public-verification-pending')
        self.assertEqual(sum(call.args[0] == 'restart' for call in run.call_args_list), 1)

    def test_resume_does_not_treat_later_user_deletions_as_deployment_data_loss(self):
        d.atomic_json(self.root / 'baseline.json', {'sessions': [{'id': 'deleted-later'}]})
        d.atomic_json(self.root / 'drafts.json', [{'session_id': 'deleted-later', 'revision': 1, 'parts': []}])
        self.obj.m.update(protocol_version=11, server_executable=str(Path('/proc', str(os.getpid()), 'exe').resolve()))
        with patch.object(self.obj, 'ready', return_value={'protocol_version': 11, 'sessions': []}), patch.object(self.obj, 'schema', return_value=18), patch.object(self.obj, 'get_json', return_value={'revision': 'new'}) as get_json, patch.object(d.subprocess, 'check_output', return_value=str(os.getpid()).encode()), patch.object(self.obj, 'public'), patch.object(self.obj, 'run') as run:
            self.assertEqual(self.obj.verify(), 0)
            get_json.assert_called_once_with('local', '/v1/version')
            run.assert_not_called()
        self.assertEqual(self.obj.state['phase'], 'deployed')
        self.assertEqual(self.active.resolve(), self.candidate)

    def test_realistic_nix_wrapper_target(self):
        server = self.root / 'relay-server'
        server.touch()
        self.obj.m['server_executable'] = str(server)
        wrapper = self.root / 'relay-start'
        wrapper.write_text('#!/nix/store/bash/bin/bash\nset -euo pipefail\n'
                           'unset RELAY_TOKEN RELAY_SETUP_TOKEN\n'
                           'export RELAY_TOKEN_FILE="$CREDENTIALS_DIRECTORY/relay-token"\n'
                           'export RELAY_SETUP_TOKEN_FILE="$CREDENTIALS_DIRECTORY/relay-setup"\n'
                           'exec ' + str(server) + '\n')
        unit = 'Environment=RELAY_WEB_ROOT=/web\nExecStart=' + str(wrapper)
        self.obj.check_unit(unit, Path('/web'))
        wrapper.write_text('#!/bin/sh\nexec /wrong/server\nexec ' + str(server) + '\n')
        with self.assertRaises(d.Failure):
            self.obj.check_unit(unit, Path('/web'))
        wrapper.write_text('#!/bin/sh\nexec ' + str(server) + '-wrong\n')
        with self.assertRaises((d.Failure, FileNotFoundError)):
            self.obj.check_unit(unit, Path('/web'))

    def test_duplicate_activate_preserves_prior_state(self):
        for phase in ('deployed', 'public-verification-pending'):
            self.obj.save(phase)
            original = self.obj.status_path.read_bytes()
            with patch.object(self.obj, 'run') as run, patch.object(self.obj, 'link') as link:
                with self.assertRaises(d.Precondition) as result:
                    self.obj.activate()
                self.obj.failure(result.exception)
                run.assert_not_called()
                link.assert_not_called()
            self.assertEqual(self.obj.status_path.read_bytes(), original)
            with patch.object(d, 'Deployment', return_value=self.obj), patch.object(sys, 'argv', ['deploy', 'activate', '--manifest', '/not-read']), patch.object(self.obj, 'reload'), patch.object(self.obj, 'run') as run, patch.object(self.obj, 'link') as link:
                self.assertEqual(d.main(), 1)
                run.assert_not_called()
                link.assert_not_called()
            self.assertEqual(self.obj.status_path.read_bytes(), original)
            self.assertEqual(self.active.resolve(), self.candidate)

    def test_status_ignores_activation_lock(self):
        self.obj.save('public-verification-pending')
        with patch.object(d, 'Deployment', return_value=self.obj), patch.object(sys, 'argv', ['deploy', 'status', '--manifest', '/not-read']), patch.object(d.fcntl, 'flock', side_effect=AssertionError('must not lock')), patch('sys.stdout', new_callable=io.StringIO) as output:
            self.assertEqual(d.main(), 0)
            self.assertEqual(json.loads(output.getvalue())['phase'], 'public-verification-pending')
            self.assertEqual(json.loads(output.getvalue())['revision'], 'new')

    def test_mutation_reloads_after_lock(self):
        with patch.object(d, 'Deployment', return_value=self.obj), patch.object(sys, 'argv', ['deploy', 'verify', '--manifest', '/not-read']), patch.object(d.fcntl, 'flock'), patch.object(self.obj, 'reload') as reload, patch.object(self.obj, 'verify', return_value=0) as verify:
            self.assertEqual(d.main(), 0)
            reload.assert_called_once()
            verify.assert_called_once()

    def test_protected_paths_absolute_and_outside_checkout(self):
        path = self.root / 'manifest'
        path.write_text('{}')
        path.chmod(0o600)
        self.assertEqual(d.protected(path), path)
        with self.assertRaises(d.Failure):
            d.protected('relative-path')
        (self.root / '.git').mkdir()
        with self.assertRaises(d.Failure):
            d.protected(path)
        with self.assertRaises(d.Failure):
            d.protected(self.root, True)

    def test_safe_failure_check_code(self):
        self.obj.state = {}
        self.obj.failure(d.Failure('public-revision'))
        self.assertEqual(self.obj.state['check'], 'public-revision')
        self.obj.failure(d.Failure('arbitrary secret'))
        self.assertEqual(self.obj.state['check'], 'internal')

    def test_tls_eof_classified_as_transport(self):
        self.obj.m.update(public_url='https://example.invalid', timeout=1)
        with patch.object(d.urllib.request.OpenerDirector, 'open', side_effect=d.ssl.SSLEOFError('secret')), patch.object(self.obj, 'curl', side_effect=d.Transport()):
            with self.assertRaises(d.Transport):
                self.obj.request('public', '/app/')

    def test_http_auth_is_not_transport(self):
        self.obj.m['public_url'] = 'https://example.invalid'
        with patch.object(d.urllib.request.OpenerDirector, 'open', side_effect=urllib.error.HTTPError('secret', 401, 'secret', {}, None)), patch.object(self.obj, 'curl') as curl:
            with self.assertRaises(d.Failure) as result:
                self.obj.request('public', '/app/')
            self.assertNotIsInstance(result.exception, d.Transport)
            curl.assert_not_called()

    def test_public_gateway_errors_retry_but_local_and_application_errors_fail(self):
        self.obj.m.update(public_url='https://example.invalid', local_url='http://127.0.0.1')
        for endpoint, status, retry in [('public', 502, True), ('public', 503, True),
                                        ('public', 504, True), ('public', 429, True),
                                        ('public', 500, False), ('public', 403, False),
                                        ('local', 503, False)]:
            with self.subTest(endpoint=endpoint, status=status), patch.object(
                    d.urllib.request.OpenerDirector, 'open',
                    side_effect=urllib.error.HTTPError('redacted', status, 'redacted', {}, None)):
                with self.assertRaises(d.Failure) as result:
                    self.obj.request(endpoint, '/v1/version')
                self.assertEqual(isinstance(result.exception, d.Transport), retry)

    def test_curl_gateway_response_is_pending_rather_than_application_failure(self):
        response = d.subprocess.CompletedProcess([], 0, stdout=b'503\nhttps://example.invalid/app/')
        with patch.object(d.subprocess, 'run', return_value=response):
            with self.assertRaises(d.Transport):
                self.obj.curl('https://example.invalid/app/', {})

    def test_local_failure_rolls_back(self):
        with patch.object(self.obj, 'schema', return_value=18), patch.object(self.obj, 'run') as run, patch.object(self.obj, 'ready'), patch.object(self.obj, 'get_json', return_value={'revision': 'old'}):
            self.obj.failure(d.Failure('local-startup'))
        self.assertEqual(self.active.resolve(), self.previous)
        self.assertTrue(self.obj.state['restored_previous'])
        self.assertEqual(run.call_count, 2)

    def test_concurrent_bundle_not_rolled_back(self):
        other = self.root / 'other'
        other.mkdir()
        self.obj.link(other)
        with patch.object(self.obj, 'run') as run:
            self.obj.failure(d.Failure('local-startup'))
            run.assert_not_called()
        self.assertEqual(self.active.resolve(), other)
        with self.assertRaises(d.Ownership):
            self.obj.verify()

    def test_schema_migration_not_rolled_back(self):
        with patch.object(self.obj, 'schema', return_value=19), patch.object(self.obj, 'run') as run:
            self.obj.failure(d.Failure('schema'))
            run.assert_not_called()
        self.assertEqual(self.active.resolve(), self.candidate)

    def migration(self, migrate=True, keep_drafts=True):
        """Prepare a real schema 21 database whose candidate migrates it to 23 on restart."""
        self.obj.link(self.previous)
        self.obj.state = {}
        database = self.root / 'workspace.sqlite3'
        with sqlite3.connect(database) as db:
            db.execute('CREATE TABLE workspace (id INTEGER PRIMARY KEY, snapshot TEXT)')
            db.execute("INSERT INTO workspace VALUES (1, '{}')")
            db.execute('PRAGMA user_version = 21')
        db.close()
        self.obj.m.update(database=str(database), protocol_version=14, schema_version=23,
                          previous_schema_version=21,
                          server_executable=str(Path('/proc', str(os.getpid()), 'exe').resolve()))
        web = Path(self.obj.m['web_root'])
        web.mkdir()
        (web / 'app_bg.wasm').write_bytes(b'new')
        (self.candidate / 'relay.service').write_text(
            'Environment=RELAY_WEB_ROOT=' + str(web) + '\nExecStart=' + self.obj.m['server_executable'])

        def run(*args):
            if migrate and args[0] == 'restart' and self.active.resolve() == self.candidate:
                with sqlite3.connect(database) as db:
                    db.execute('PRAGMA user_version = 23')
                db.close()
        snapshot = {'protocol_version': 14, 'sessions': [{'id': 'kept'}]}

        def get_json(endpoint, path):
            if path == '/v1/drafts':
                draft = {'session_id': 'kept', 'revision': 2, 'parts': []}
                return [draft] if keep_drafts or self.active.resolve() == self.previous else []
            return {'revision': 'new' if self.active.resolve() == self.candidate else 'old'}
        return [patch.object(self.obj, 'run', side_effect=run), patch.object(self.obj, 'ready', return_value=snapshot),
                patch.object(self.obj, 'get_json', side_effect=get_json),
                patch.object(d.subprocess, 'check_output', return_value=str(os.getpid()).encode())]

    def backup_schema(self):
        with closing(sqlite3.connect(self.root / 'database-backup.sqlite3')) as db:
            return db.execute('PRAGMA user_version').fetchone()[0]

    def test_schema_versions_default_and_range(self):
        self.assertEqual(d.schema_versions({'schema_version': 18}), (18, 18))
        self.assertEqual(d.schema_versions({'schema_version': 23, 'previous_schema_version': 21}), (21, 23))
        for previous in (24, True, '21', None, -1):
            with self.subTest(previous=previous), self.assertRaises(d.Failure) as result:
                d.schema_versions({'schema_version': 23, 'previous_schema_version': previous})
            self.assertEqual(result.exception.args, ('schema-range',))
        with self.assertRaises(d.Failure):
            d.schema_versions({'schema_version': '23'})

    def test_migration_activates_from_previous_schema_and_verifies_target(self):
        patches = self.migration()
        with patches[0] as run, patches[1], patches[2], patches[3], patch.object(self.obj, 'public'):
            self.assertEqual(self.obj.activate(), 0)
        self.assertEqual(self.obj.state['phase'], 'deployed')
        self.assertEqual(self.active.resolve(), self.candidate)
        self.assertEqual(self.obj.schema(), 23)
        self.assertEqual(self.backup_schema(), 21)
        self.assertEqual(sum(call.args[0] == 'restart' for call in run.call_args_list), 1)

    def test_pre_schema_compares_with_previous_schema(self):
        for manifest, live in (({'previous_schema_version': 21}, 23), ({'previous_schema_version': None}, 21)):
            with self.subTest(manifest=manifest, live=live):
                self.setUp()
                patches = self.migration()
                if manifest['previous_schema_version'] is None:
                    del self.obj.m['previous_schema_version']
                with closing(sqlite3.connect(self.obj.m['database'])) as db:
                    db.execute('PRAGMA user_version = %d' % live)
                with patches[0] as run, patches[1], patches[2], patches[3]:
                    with self.assertRaises(d.Failure) as result:
                        self.obj.activate()
                    self.obj.failure(result.exception)
                    run.assert_not_called()
                self.assertEqual(self.obj.state['check'], 'pre-schema')
                self.assertEqual(self.active.resolve(), self.previous)
                self.assertFalse((self.root / 'database-backup.sqlite3').exists())

    def test_local_check_requires_target_schema_after_activation(self):
        patches = self.migration(migrate=False)
        with patches[0] as run, patches[1], patches[2], patches[3], patch.object(self.obj, 'public') as public:
            with self.assertRaises(d.Failure) as result:
                self.obj.activate()
            self.assertEqual(result.exception.args, ('schema-protocol',))
            public.assert_not_called()
            # The candidate never migrated, so the previous bundle can still run on schema 21.
            self.obj.failure(result.exception)
        self.assertEqual(self.active.resolve(), self.previous)
        self.assertTrue(self.obj.state['restored_previous'])
        self.assertEqual(self.obj.schema(), 21)
        self.assertEqual(sum(call.args[0] == 'restart' for call in run.call_args_list), 2)

    def test_failure_after_migration_never_switches_back(self):
        patches = self.migration(keep_drafts=False)
        with patches[0] as run, patches[1], patches[2], patches[3]:
            with self.assertRaises(d.Failure) as result:
                self.obj.activate()
            self.obj.failure(result.exception)
        self.assertEqual(self.obj.state['phase'], 'failed')
        self.assertEqual(self.obj.state['check'], 'drafts')
        self.assertFalse(self.obj.state['restored_previous'])
        self.assertEqual(self.active.resolve(), self.candidate)
        self.assertEqual(self.obj.schema(), 23)
        self.assertEqual(self.backup_schema(), 21)
        self.assertEqual(sum(call.args[0] == 'restart' for call in run.call_args_list), 1)
        self.assertFalse([call for call in run.call_args_list if call.args[0] == 'start'])

    def test_public_revision_and_asset_mismatch_are_hard(self):
        web = Path(self.obj.m['web_root'])
        web.mkdir()
        (web / 'index.html').write_bytes(b'expected')
        for revision in ('wrong', 'new'):
            with self.subTest(revision=revision), patch.object(self.obj, 'request', side_effect=[(b'<form', 'https://example.invalid/login'), (b'wrong asset', 'https://example.invalid/app/index.html')]), patch.object(self.obj, 'get_json', side_effect=[{'revision': revision}, {}]):
                with self.assertRaises(d.Failure) as result:
                    self.obj.public()
                self.assertNotIsInstance(result.exception, d.Transport)
        self.assertNotEqual(self.obj.state.get('phase'), 'deployed')

    def test_atomic_status_redacts_exception(self):
        with patch.object(self.obj, 'schema', return_value=19):
            self.obj.failure(d.Failure('TOKEN secret https://user:password@example.invalid'))
        self.assertNotIn('secret', self.obj.status_path.read_text())
        self.assertEqual(self.obj.status_path.stat().st_mode & 0o777, 0o600)
        original = self.obj.status_path.read_bytes()
        with patch.object(d.os, 'replace', side_effect=OSError('failed')):
            with self.assertRaises(OSError):
                d.atomic_json(self.obj.status_path, {'phase': 'new'})
        self.assertEqual(self.obj.status_path.read_bytes(), original)
        self.assertFalse(list(self.root.glob('.status-*')))


if __name__ == '__main__':
    unittest.main()
