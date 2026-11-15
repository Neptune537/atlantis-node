import urllib.request, urllib.error, sys, json
base = sys.argv[1] if len(sys.argv)>1 else 'http://127.0.0.1:8087'
origin = 'https://heavenlysent-680ac.web.app'
def check(label, path, method, body, request_origin, expected):
    headers={'User-Agent':'Mozilla/5.0','Content-Type':'application/json','Origin':request_origin}
    if method=='OPTIONS': headers.update({'Access-Control-Request-Method':'POST','Access-Control-Request-Headers':'content-type,authorization'})
    req=urllib.request.Request(base+path, data=body, method=method, headers=headers)
    try:
        response=urllib.request.urlopen(req,timeout=20)
    except urllib.error.HTTPError as e: response=e
    data=response.read(300)
    status=response.code
    cors=response.headers.get('Access-Control-Allow-Origin')
    print(label,status,'CORS='+str(cors),data.decode(errors='replace'))
    assert status in expected, (label,status)
    if request_origin==origin: assert cors==origin,(label,'missing browser CORS')
check('unsigned profile','/api/auth/me','GET',None,origin,[401])
check('invalid login','/api/auth/oidc','POST',b'{"id_token":"invalid"}',origin,[401])
check('browser preflight','/api/auth/oidc','OPTIONS',None,origin,[200,204])
check('untrusted browser','/api/auth/me','GET',None,'https://untrusted.example',[403])
check('key upload forbidden','/api/auth/jwks-sync','POST',b'{"jwks_json":"{}"}',origin,[403])
if base.startswith('http://127.0.0.1:'):
    check('no financial router','/api/ledger/audit','GET',None,origin,[404])
print('All auth ingress checks passed')
