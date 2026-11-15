"""Install the narrow auth ingress rule after the service passes local checks."""
from pathlib import Path
import shutil, subprocess, urllib.request, urllib.error
p = Path('/home/zero/.cloudflared/config.yml')
assert p.resolve() == p
try:
    urllib.request.urlopen('http://127.0.0.1:8087/api/auth/me', timeout=5)
    raise RuntimeError('Expected unauthenticated 401')
except urllib.error.HTTPError as e:
    assert e.code == 401, e.code
s = p.read_text()
rule = ('  - hostname: tr7n7t7.com\n    path: ^/api/auth/club/(presence|leave|messages)$\n'
        '    service: http://127.0.0.1:8087\n'
        '  - hostname: tr7n7t7.com\n    path: ^/api/auth/(oidc|me|logout|wallet-challenge|link-wallet|align|jwks-sync)$\n'
        '    service: http://127.0.0.1:8087\n')
if rule in s:
    print('Auth rule already installed')
    raise SystemExit(0)
marker = '  - hostname: tr7n7t7.com\n'
assert s.count(marker) == 1, 'Unexpected ingress layout; manual review required'
backup = p.with_name('config.before-auth-20261111.yml')
assert not backup.exists(), 'Backup exists; inspect before overwriting'
shutil.copy2(p, backup)
try:
    p.write_text(s.replace(marker, rule + marker, 1))
    subprocess.run(['/home/zero/cloudflared','tunnel','--config',str(p),'ingress','validate'],check=True)
    subprocess.run(['/home/zero/cloudflared','tunnel','--config',str(p),'ingress','rule','https://tr7n7t7.com/api/auth/me'],check=True)
    subprocess.run(['systemctl','--user','restart','cloudflared-radio.service'],check=True)
except Exception:
    shutil.copy2(backup,p)
    subprocess.run(['systemctl','--user','restart','cloudflared-radio.service'],check=False)
    raise
print('Auth ingress installed; prior tunnel config preserved at',backup)
