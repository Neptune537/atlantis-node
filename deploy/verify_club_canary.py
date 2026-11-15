"""Exercise two test sessions against the isolated Pi canary database only."""
import hashlib
import json
import secrets
import sqlite3
import time
import urllib.error
import urllib.request
from pathlib import Path

DB = Path('/home/zero/atlantis-auth/club-canary-data/auth-sessions.sqlite')
BASE = 'http://127.0.0.1:8088/api/auth/club'
ORIGIN = 'https://play.tr7n7t7.com'
now = int(time.time())
principals = ['did:atlantis:canary-alice', 'did:atlantis:canary-bob']
tokens = [secrets.token_hex(32) for _ in principals]


def request(path, token, body=None):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(BASE + path, data=data, headers={
        'Authorization': 'Bearer ' + token, 'Origin': ORIGIN,
        'Content-Type': 'application/json',
    })
    with urllib.request.urlopen(req, timeout=5) as response:
        return response.status, json.loads(response.read()) if response.status != 204 else None


try:
    with sqlite3.connect(DB) as db:
        for principal, token in zip(principals, tokens):
            db.execute('INSERT INTO auth_accounts(principal,created,last_login) VALUES (?,?,?)',
                       (principal, now, now))
            db.execute('INSERT INTO auth_sessions(token_hash,principal,expires,last_seen) VALUES (?,?,?,?)',
                       (hashlib.sha256(token.encode()).hexdigest(), principal, now + 600, now))
    assert request('/presence', tokens[0], {'scene': 'Atlantis:surface'})[0] == 204
    assert request('/presence', tokens[1], {'scene': 'Atlantis:surface'})[0] == 204
    status, presence = request('/presence?scene=Atlantis:surface', tokens[0])
    assert status == 200 and {p['principal_did'] for p in presence} == set(principals)
    assert request('/messages', tokens[0], {'scene': 'Atlantis:surface', 'text': 'Canary hello'})[0] == 200
    status, messages = request('/messages?scene=Atlantis:surface', tokens[1])
    assert status == 200 and any(m['text'] == 'Canary hello' for m in messages)
    assert request('/leave', tokens[1], {})[0] == 204
    try:
        request('/messages', tokens[1], {'scene': 'Atlantis:surface', 'text': 'Should fail'})
        raise AssertionError('Message after leaving was accepted')
    except urllib.error.HTTPError as error:
        assert error.code == 400
    print('Canary two-session presence, chat, leave and posting guard passed.')
finally:
    with sqlite3.connect(DB) as db:
        db.executemany('DELETE FROM auth_sessions WHERE principal=?', [(p,) for p in principals])
        db.executemany('DELETE FROM club_presence WHERE principal=?', [(p,) for p in principals])
        db.executemany('DELETE FROM club_messages WHERE principal=?', [(p,) for p in principals])
        db.executemany('DELETE FROM auth_accounts WHERE principal=?', [(p,) for p in principals])
