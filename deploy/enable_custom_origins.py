from pathlib import Path
import subprocess,shutil,time,urllib.request,urllib.error,json
root=Path('/home/zero/atlantis-auth')
current='atlantis-auth-api'
backup='atlantis-auth-api-before-custom-origins'
info=json.loads(subprocess.check_output(['docker','inspect',current]))[0]
origin_env=next(value for value in info['Config']['Env'] if value.startswith('ATLANTIS_AUTH_ORIGINS='))
if 'https://lite.tr7n7t7.com' in origin_env and 'https://play.tr7n7t7.com' in origin_env:
    print('Custom auth origins already enabled');raise SystemExit(0)
if subprocess.run(['docker','inspect',backup],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL).returncode==0:
    raise RuntimeError('Rollback container already exists; inspect before replacing')
script=root/'start_auth_seed_pi.sh'
saved=root/'start_auth_seed_pi.before-custom-origins.sh'
assert not saved.exists()
shutil.copy2(script,saved)
shutil.copy2(root/'start_auth_seed_pi.new.sh',script)
subprocess.run(['docker','rename',current,backup],check=True)
subprocess.run(['docker','stop',backup],check=True)
try:
    subprocess.run(['sh',str(script)],check=True)
    for attempt in range(20):
        try:
            req=urllib.request.Request('http://127.0.0.1:8087/api/auth/me',headers={'Origin':'https://lite.tr7n7t7.com'})
            urllib.request.urlopen(req,timeout=2)
        except urllib.error.HTTPError as error:
            if error.code==401 and error.headers.get('Access-Control-Allow-Origin')=='https://lite.tr7n7t7.com':break
        except OSError:pass
        time.sleep(1)
    else:raise RuntimeError('New auth service failed readiness/CORS checks')
except Exception:
    subprocess.run(['docker','stop',current],check=False)
    subprocess.run(['docker','rename',current,'atlantis-auth-api-failed-custom-origins'],check=False)
    subprocess.run(['docker','rename',backup,current],check=True)
    shutil.copy2(saved,script)
    subprocess.run(['docker','start',current],check=True)
    raise
print('Custom origins enabled; original container and startup script retained for rollback')
