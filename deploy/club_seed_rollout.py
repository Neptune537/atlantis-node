"""Run on the seed Pi: prepare and cut over the isolated Club auth API."""
import json
import shutil
import sqlite3
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path('/home/zero/atlantis-auth')
OLD = ROOT / 'releases/20261111'
NEW = ROOT / 'releases/20261113-club'
DATA = ROOT / 'data'
DB = DATA / 'auth-sessions.sqlite'
BACKUP = DATA / 'auth-sessions.before-club-20261113.sqlite'
CANARY_DATA = ROOT / 'club-canary-data'
CONFIG = Path('/home/zero/.cloudflared/config.yml')
CONFIG_BACKUP = CONFIG.with_name('config.before-club-20261113.yml')
OLD_RULE = 'path: ^/api/auth/(oidc|me|logout|wallet-challenge|link-wallet|align|jwks-sync)$'
NEW_RULE = 'path: ^/api/auth/(oidc|me|logout|wallet-challenge|link-wallet|align|jwks-sync|club/(presence|leave|messages))$'


def run(*args):
    return subprocess.run(args, check=True, text=True, capture_output=True).stdout.strip()


def check(url, expected, origin=None):
    if url.startswith('https://'):
        args = ['curl', '-sS', '--max-time', '8', '-o', '/dev/null', '-w', '%{http_code}']
        if '/live.mp3' in url:
            args.append('-I')
        if origin:
            args += ['-H', 'Origin: ' + origin]
        status = int(run(*args, url))
    else:
        request = urllib.request.Request(url, headers=({'Origin': origin} if origin else {}))
        try:
            with urllib.request.urlopen(request, timeout=8) as response:
                status = response.status
        except urllib.error.HTTPError as error:
            status = error.code
    if status != expected:
        raise RuntimeError(f'{url}: expected {expected}, got {status}')
    print(f'{url}: {status}')


def wait_check(url, expected, origin=None):
    for attempt in range(30):
        try:
            return check(url, expected, origin)
        except (urllib.error.URLError, TimeoutError, ConnectionError, RuntimeError):
            if attempt == 29:
                raise
            time.sleep(1)


def inspect_current():
    info = json.loads(run('docker', 'inspect', 'atlantis-auth-api'))[0]
    assert info['State']['Running'] and info['Config']['Image'] == 'rust:latest'
    assert any(m['Source'] == str(OLD) and m['Destination'] == '/work' for m in info['Mounts'])
    assert any(m['Source'] == str(DATA) and m['Destination'] == '/data' for m in info['Mounts'])
    env = [value for value in info['Config']['Env'] if value.startswith(('ATLANTIS_', 'GOOGLE_OAUTH_CLIENT_ID='))]
    assert len(env) == 4
    return env


def start(name, release, data, port, env):
    args = ['docker', 'run', '-d', '--name', name, '--restart', 'unless-stopped' if port == 8087 else 'no',
            '--user', '1000:1000', '--read-only', '--tmpfs', '/tmp:rw,noexec,nosuid,size=16m',
            '--cap-drop', 'ALL', '--security-opt', 'no-new-privileges',
            '-p', f'127.0.0.1:{port}:8087']
    for value in env:
        args += ['-e', value]
    args += ['-v', f'{release}:/work:ro', '-v', f'{data}:/data', 'rust:latest', '/work/target/debug/auth_api']
    run(*args)


def prepare():
    env = inspect_current()
    assert (NEW / 'target/debug/auth_api').is_file()
    if not BACKUP.exists():
        with sqlite3.connect(DB) as source, sqlite3.connect(BACKUP) as target:
            source.backup(target)
        BACKUP.chmod(0o600)
    if not CANARY_DATA.exists():
        CANARY_DATA.mkdir(mode=0o700)
        with sqlite3.connect(DB) as source, sqlite3.connect(CANARY_DATA / DB.name) as target:
            source.backup(target)
        (CANARY_DATA / DB.name).chmod(0o600)
    if subprocess.run(['docker', 'inspect', 'atlantis-auth-api-club-canary'], capture_output=True).returncode != 0:
        start('atlantis-auth-api-club-canary', NEW, CANARY_DATA, 8088, env)
    wait_check('http://127.0.0.1:8088/api/auth/me', 401)
    check('http://127.0.0.1:8088/api/auth/club/presence?scene=Atlantis:surface', 401)
    check('http://127.0.0.1:8088/api/auth/club/messages?scene=Atlantis:surface', 401)
    check('http://127.0.0.1:8088/api/auth/club/messages?scene=Atlantis:surface', 403, 'https://evil.example')
    config = CONFIG.read_text()
    assert config.count(OLD_RULE) == 1 and NEW_RULE not in config
    if not CONFIG_BACKUP.exists():
        shutil.copy2(CONFIG, CONFIG_BACKUP)
    candidate = CONFIG.with_name('config.club-candidate.yml')
    candidate.write_text(config.replace(OLD_RULE, NEW_RULE))
    run('/home/zero/cloudflared', 'tunnel', '--config', str(candidate), 'ingress', 'validate')
    assert '127.0.0.1:8087' in run('/home/zero/cloudflared', 'tunnel', '--config', str(candidate),
                                    'ingress', 'rule', 'https://tr7n7t7.com/api/auth/club/presence')
    print('Canary and tunnel candidate ready; running auth service unchanged.')


def cutover():
    env = inspect_current()
    assert CONFIG_BACKUP.is_file()
    canary_rule = ('  - hostname: tr7n7t7.com\n'
                   '    path: ^/api/auth/club/(presence|leave|messages)$\n'
                   '    service: http://127.0.0.1:8088\n')
    live_rule = canary_rule.replace(':8088', ':8087')
    config = CONFIG.read_text()
    assert config.count(canary_rule) == 1 and config.count(OLD_RULE) == 1
    candidate = CONFIG.with_name('config.club-live-candidate.yml')
    candidate.write_text(config.replace(canary_rule, live_rule))
    run('/home/zero/cloudflared', 'tunnel', '--config', str(candidate), 'ingress', 'validate')
    assert '127.0.0.1:8087' in run('/home/zero/cloudflared', 'tunnel', '--config', str(candidate),
                                    'ingress', 'rule', 'https://tr7n7t7.com/api/auth/club/presence')
    check('http://127.0.0.1:8088/api/auth/club/presence?scene=Atlantis:surface', 401)
    switched = False
    try:
        run('docker', 'stop', 'atlantis-auth-api')
        run('docker', 'rename', 'atlantis-auth-api', 'atlantis-auth-api-before-club')
        switched = True
        start('atlantis-auth-api', NEW, DATA, 8087, env)
        wait_check('http://127.0.0.1:8087/api/auth/me', 401)
        check('http://127.0.0.1:8087/api/auth/club/presence?scene=Atlantis:surface', 401,
              'https://play.tr7n7t7.com')
        shutil.copy2(candidate, CONFIG)
        run('systemctl', '--user', 'restart', 'cloudflared-radio.service')
        wait_check('https://tr7n7t7.com/api/auth/me', 401, 'https://play.tr7n7t7.com')
        wait_check('https://tr7n7t7.com/api/auth/club/presence?scene=Atlantis:surface', 401,
              'https://play.tr7n7t7.com')
        wait_check('https://tr7n7t7.com/api/auth/club/messages?scene=Atlantis:surface', 401,
              'https://lite.tr7n7t7.com')
        wait_check('https://radio.tr7n7t7.com/live.mp3', 200)
    except Exception:
        if CONFIG_BACKUP.exists():
            shutil.copy2(CONFIG_BACKUP, CONFIG)
            subprocess.run(['systemctl', '--user', 'restart', 'cloudflared-radio.service'], check=False)
        if switched:
            subprocess.run(['docker', 'stop', 'atlantis-auth-api'], check=False, capture_output=True)
            subprocess.run(['docker', 'rm', 'atlantis-auth-api'], check=False, capture_output=True)
            subprocess.run(['docker', 'rename', 'atlantis-auth-api-before-club', 'atlantis-auth-api'], check=False)
            subprocess.run(['docker', 'start', 'atlantis-auth-api'], check=False)
        raise
    print('Club auth API and narrow ingress active; prior container retained for rollback.')


if __name__ == '__main__':
    {'prepare': prepare, 'cutover': cutover}[sys.argv[1]]()
