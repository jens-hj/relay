#!/usr/bin/env python3
"""Detached, resumable Relay deployment. See --help; never prints exception text."""
import argparse
from contextlib import closing
import fcntl
import hashlib
import json
import os
from pathlib import Path
import sqlite3
import shlex
import ssl
import stat
import subprocess
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request


class Failure(Exception):
    category = 'application'


class Precondition(Failure):
    category = 'precondition'


class Transport(Failure):
    category = 'public-transport'


class Ownership(Failure):
    category = 'ownership'


PUBLIC_GATEWAY_STATUS = {408, 429, 502, 503, 504}


def protected(path, directory=False):
    path = Path(path)
    if not path.is_absolute():
        raise Failure('absolute-path')
    resolved = path.resolve()
    if any((parent / '.git').exists() for parent in (resolved, *resolved.parents)):
        raise Failure('checkout-path')
    mode = path.stat()
    if mode.st_uid != os.getuid() or stat.S_IMODE(mode.st_mode) != (0o700 if directory else 0o600):
        raise Failure('permissions')
    if path.is_symlink() or (directory and not path.is_dir()) or (not directory and not path.is_file()):
        raise Failure('file-type')
    return path


def atomic_json(path, value):
    fd, temporary = tempfile.mkstemp(dir=path.parent, prefix='.status-')
    try:
        with os.fdopen(fd, 'w') as stream:
            json.dump(value, stream)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
        fd = os.open(path.parent, os.O_DIRECTORY)
        try:
            os.fsync(fd)
        finally:
            os.close(fd)
    finally:
        Path(temporary).unlink(missing_ok=True)


class SafeRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, fp, code, msg, headers, newurl):
        old = urllib.parse.urlsplit(request.full_url)
        new = urllib.parse.urlsplit(newurl)
        if (old.scheme, old.netloc) != (new.scheme, new.netloc):
            raise Failure('redirect-origin')
        return super().redirect_request(request, fp, code, msg, headers, newurl)


class Deployment:
    def __init__(self, manifest):
        manifest = protected(manifest)
        protected(manifest.parent, True)
        self.m = json.loads(manifest.read_text())
        self.manifest_digest = hashlib.sha256(manifest.read_bytes()).hexdigest()
        self.root = protected(self.m['state_dir'], True)
        protected(self.m['credential_file'])
        protected(Path(self.m['credential_file']).parent, True)
        for key in ('local_url', 'public_url'):
            url = urllib.parse.urlsplit(self.m[key])
            if url.username or url.password or url.query or url.fragment or url.path not in ('', '/'):
                raise Failure('url')
            if url.scheme != ('https' if key == 'public_url' else 'http'):
                raise Failure('url')
            if key == 'local_url' and url.hostname not in ('127.0.0.1', 'localhost', '::1'):
                raise Failure('loopback')
        for key in ('active_link', 'candidate_bundle', 'previous_bundle', 'database', 'server_executable', 'web_root'):
            if not Path(self.m[key]).is_absolute():
                raise Failure('absolute-path')
        self.status_path = self.root / 'status.json'
        self.reload()

    def reload(self):
        self.state = json.loads(protected(self.status_path).read_text()) if self.status_path.exists() else {}
        if self.state and (self.state.get('revision') != self.m['revision'] or self.state.get('manifest_digest') != self.manifest_digest):
            raise Ownership('attempt')

    def save(self, phase, **values):
        if phase == 'deployed':
            for key in ('category', 'check', 'restored_previous'):
                self.state.pop(key, None)
        self.state.update(phase=phase, revision=self.m['revision'],
                          manifest_digest=self.manifest_digest, **values)
        atomic_json(self.status_path, self.state)

    def run(self, *args):
        subprocess.run([self.m.get('systemctl', 'systemctl'), '--user', *args], check=True,
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=120)

    def active(self):
        return Path(self.m['active_link']).resolve(strict=True)

    def own(self):
        if self.active() != Path(self.m['candidate_bundle']).resolve(strict=True):
            raise Ownership('active-bundle')

    def link(self, target):
        path = Path(self.m['active_link'])
        temporary = path.with_name(path.name + '.next-' + str(os.getpid()))
        try:
            temporary.symlink_to(target)
            os.replace(temporary, path)
        finally:
            temporary.unlink(missing_ok=True)

    def schema(self):
        with closing(sqlite3.connect('file:' + self.m['database'] + '?mode=ro', uri=True)) as db:
            if db.execute('PRAGMA quick_check').fetchone()[0] != 'ok':
                raise Failure('database-integrity')
            return db.execute('PRAGMA user_version').fetchone()[0]

    def request(self, endpoint, path, auth=False):
        url = self.m[endpoint + '_url'].rstrip('/') + path
        headers = {'Accept-Encoding': 'identity'}
        if auth:
            headers['Authorization'] = 'Bearer ' + protected(self.m['credential_file']).read_text().strip()
        try:
            with urllib.request.build_opener(SafeRedirect()).open(urllib.request.Request(url, headers=headers), timeout=self.m.get('timeout', 15)) as response:
                return response.read(), response.geturl()
        except urllib.error.HTTPError as error:
            if endpoint == 'public' and error.code in PUBLIC_GATEWAY_STATUS:
                raise Transport('public-gateway') from None
            raise Failure('http-status') from None
        except (urllib.error.URLError, TimeoutError, ConnectionError, ssl.SSLError):
            if endpoint == 'public':
                return self.curl(url, headers)
            raise Failure('local-transport') from None

    def curl(self, url, headers):
        with tempfile.TemporaryDirectory(dir=self.root) as temporary:
            config = Path(temporary) / 'config'
            body = Path(temporary) / 'body'
            config.write_text('\n'.join('header = ' + json.dumps(k + ': ' + v) for k, v in headers.items()))
            config.chmod(0o600)
            try:
                result = subprocess.run([self.m.get('curl', 'curl'), '--config', str(config), '--silent',
                    '--location', '--proto', '=https', '--proto-redir', '=https', '--max-time',
                    str(self.m.get('timeout', 15)), '--output', str(body), '--write-out', '%{http_code}\n%{url_effective}', url],
                    stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, timeout=self.m.get('timeout', 15) + 5)
            except (subprocess.TimeoutExpired, OSError):
                raise Transport('curl-transport') from None
            if result.returncode:
                # Only known network/TLS failures are retryable; configuration and HTTP failures are hard.
                if result.returncode in (5, 6, 7, 18, 28, 35, 52, 55, 56, 60):
                    raise Transport('curl-transport')
                raise Failure('curl-failure')
            code, effective = result.stdout.decode().split('\n', 1)
            if urllib.parse.urlsplit(effective).netloc != urllib.parse.urlsplit(url).netloc:
                raise Failure('redirect-origin')
            if int(code) in PUBLIC_GATEWAY_STATUS:
                raise Transport('public-gateway')
            if not 200 <= int(code) < 300:
                raise Failure('http-status')
            return body.read_bytes(), effective

    def get_json(self, endpoint, path):
        return json.loads(self.request(endpoint, path, True)[0])

    def ready(self):
        for _ in range(self.m.get('readiness_attempts', 30)):
            try:
                return self.get_json('local', '/v1/snapshot')
            except Failure:
                time.sleep(self.m.get('retry_delay', 2))
        raise Failure('local-readiness')

    def local(self, before, drafts):
        after = self.ready()
        if after['protocol_version'] != self.m['protocol_version'] or self.schema() != self.m['schema_version']:
            raise Failure('schema-protocol')
        if before is not None:
            for key in ('projects', 'sessions', 'agent_configs', 'issues', 'messages', 'connections', 'boards'):
                if not {v['id'] for v in before.get(key, [])} <= {v['id'] for v in after.get(key, [])}:
                    raise Failure('history')
        if drafts is not None:
            current = {d['session_id']: d for d in self.get_json('local', '/v1/drafts')}
            for draft in drafts:
                value = current.get(draft['session_id'])
                if value is None or value['revision'] < draft['revision'] or (value['revision'] == draft['revision'] and value['parts'] != draft['parts']):
                    raise Failure('drafts')
        if self.get_json('local', '/v1/version')['revision'] != self.m['revision']:
            raise Failure('local-revision')
        pid = subprocess.check_output([self.m.get('systemctl', 'systemctl'), '--user', 'show', self.m.get('service', 'relay.service'), '-p', 'MainPID', '--value'], stderr=subprocess.DEVNULL, timeout=15).decode().strip()
        if not pid.isdecimal() or Path('/proc', pid, 'exe').resolve() != Path(self.m['server_executable']).resolve():
            raise Failure('executable')

    def public(self):
        login, url = self.request('public', '/app/')
        if '/login' not in urllib.parse.urlsplit(url).path or b'<form' not in login:
            raise Failure('login-gate')
        if self.get_json('public', '/v1/version')['revision'] != self.m['revision']:
            raise Failure('public-revision')
        self.get_json('public', '/v1/snapshot')
        root = Path(self.m['web_root'])
        files = [p for p in root.rglob('*') if p.is_file() and p.suffix != '.gz']
        if not files or not (root / 'index.html').is_file():
            raise Failure('assets-empty')
        for path in files:
            raw, _ = self.request('public', '/app/' + urllib.parse.quote(path.relative_to(root).as_posix()), True)
            if hashlib.sha256(raw).digest() != hashlib.sha256(path.read_bytes()).digest():
                raise Failure('public-asset')

    def verify(self):
        self.own()
        if not self.state.get('local_healthy'):
            raise Precondition('activation-incomplete')
        # Preservation was checked before recording local_healthy. Users may
        # legitimately delete sessions or drafts while public verification waits.
        self.local(None, None)
        for attempt in range(self.m.get('public_attempts', 3)):
            self.own()
            try:
                self.public()
                self.own()
                self.save('deployed')
                return 0
            except Transport as error:
                code = error.args[0] if error.args else None
                self.save('public-verification-pending', category=Transport.category,
                          check=code if code in ('public-gateway', 'curl-transport') else 'public-transport')
                if attempt + 1 < self.m.get('public_attempts', 3):
                    time.sleep(self.m.get('retry_delay', 2))
        return 2

    def check_unit(self, unit, web):
        lines = [line.strip() for line in unit.splitlines()]
        if lines.count('Environment=RELAY_WEB_ROOT=' + str(web)) != 1:
            raise Failure('unit-package')
        commands = [line.removeprefix('ExecStart=') for line in lines if line.startswith('ExecStart=')]
        if len(commands) != 1:
            raise Failure('unit-package')
        command = shlex.split(commands[0])
        if len(command) != 1 or not Path(command[0]).is_absolute():
            raise Failure('unit-package')
        expected = Path(self.m['server_executable']).resolve(strict=True)
        target = Path(command[0]).resolve(strict=True)
        if target == expected:
            return
        # Inspect the generated Nix shell wrapper as text; never execute it.
        script = target.read_text()
        statements = [line.strip() for line in script.splitlines()
                      if line.strip() and not line.lstrip().startswith('#')]
        if not script.startswith('#!') or not statements:
            raise Failure('wrapper-target')
        allowed_prelude = {
            'set -euo pipefail',
            'unset RELAY_TOKEN RELAY_SETUP_TOKEN',
            'export RELAY_TOKEN_FILE="$CREDENTIALS_DIRECTORY/relay-token"',
            'export RELAY_SETUP_TOKEN_FILE="$CREDENTIALS_DIRECTORY/relay-setup"',
        }
        if any(line not in allowed_prelude for line in statements[:-1]):
            raise Failure('wrapper-target')
        final = shlex.split(statements[-1])
        if len(final) != 2 or final[0] != 'exec' or not Path(final[1]).is_absolute():
            raise Failure('wrapper-target')
        if Path(final[1]).resolve(strict=True) != expected:
            raise Failure('wrapper-target')

    def activate(self):
        if self.state:
            raise Precondition('attempt-already-started')
        if Path(self.m['candidate_bundle']).resolve(strict=True) == Path(self.m['previous_bundle']).resolve(strict=True):
            raise Failure('identical-bundles')
        if self.active() != Path(self.m['previous_bundle']).resolve(strict=True):
            raise Ownership('previous-bundle')
        if self.schema() != self.m['schema_version']:
            raise Failure('pre-schema')
        web = Path(self.m['web_root'])
        if not any(self.m['revision'].encode() in p.read_bytes() for p in web.glob('*_bg.wasm')):
            raise Failure('frontend-revision')
        unit = (Path(self.m['candidate_bundle']) / self.m.get('service', 'relay.service')).read_text()
        self.check_unit(unit, web)
        before = self.ready()
        if before['protocol_version'] != self.m['protocol_version']:
            raise Failure('pre-protocol')
        drafts = self.get_json('local', '/v1/drafts')
        previous_revision = self.get_json('local', '/v1/version')['revision']
        atomic_json(self.root / 'baseline.json', before)
        atomic_json(self.root / 'drafts.json', drafts)
        with closing(sqlite3.connect(self.m['database'])) as source, closing(sqlite3.connect(self.root / 'database-backup.sqlite3')) as destination:
            source.backup(destination)
        self.save('prepared', previous_revision=previous_revision, stopped=False, activated=False)
        self.run('stop', self.m.get('service', 'relay.service'))
        self.save('stopped', stopped=True)
        if self.active() != Path(self.m['previous_bundle']).resolve():
            raise Ownership('concurrent-activation')
        with closing(sqlite3.connect(self.m['database'])) as source, closing(sqlite3.connect(self.root / 'database-backup.sqlite3')) as destination:
            source.backup(destination)
        self.link(self.m['candidate_bundle'])
        self.save('activated', activated=True)
        self.run('daemon-reload')
        self.run('restart', self.m.get('service', 'relay.service'))
        self.local(before, drafts)
        self.run('start', self.m.get('public_service', 'relay-funnel.service'))
        self.save('locally-healthy', local_healthy=True)
        return self.verify()

    def failure(self, error):
        if isinstance(error, Precondition):
            return
        restored = False
        # Never restore data automatically, nor run an older binary on a migrated schema.
        if not isinstance(error, Ownership) and self.state.get('stopped'):
            try:
                expected = self.m['candidate_bundle'] if self.state.get('activated') else self.m['previous_bundle']
                if self.active() == Path(expected).resolve() and self.schema() == self.m['schema_version']:
                    self.link(self.m['previous_bundle'])
                    self.run('daemon-reload')
                    self.run('restart', self.m.get('service', 'relay.service'))
                    self.ready()
                    restored = self.get_json('local', '/v1/version')['revision'] == self.state['previous_revision']
            except Exception:
                pass
        codes = {'permissions', 'file-type', 'absolute-path', 'checkout-path', 'url', 'loopback',
                 'attempt', 'active-bundle', 'database-integrity', 'redirect-origin', 'http-status',
                 'local-transport', 'curl-transport', 'curl-failure', 'local-readiness', 'schema-protocol',
                 'history', 'drafts', 'local-revision', 'executable', 'login-gate', 'public-revision',
                 'assets-empty', 'public-asset', 'identical-bundles', 'previous-bundle', 'pre-schema',
                 'frontend-revision', 'unit-package', 'wrapper-target', 'pre-protocol', 'concurrent-activation'}
        code = error.args[0] if isinstance(error, Failure) and error.args else None
        self.save('failed', category=getattr(error, 'category', 'internal'),
                  check=code if isinstance(code, str) and code in codes else 'internal',
                  restored_previous=restored)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=('activate', 'verify', 'status'))
    parser.add_argument('--manifest', required=True)
    args = parser.parse_args()
    os.umask(0o077)
    deployment = None
    try:
        deployment = Deployment(args.manifest)
        if args.command == 'status':
            status = {k: deployment.state[k] for k in ('phase', 'category', 'check', 'restored_previous') if k in deployment.state}
            status.setdefault('phase', 'not-started')
            status['revision'] = deployment.m['revision']
            print(json.dumps(status))
            return 0
        with open(Path(deployment.m['active_link']).with_name('.relay-deployment.lock'), 'a') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            deployment.reload()
            try:
                return getattr(deployment, args.command)()
            except Exception as error:
                deployment.failure(error)
                return 1
    except Exception:
        print('{"phase":"failed","category":"configuration-or-lock"}')
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
