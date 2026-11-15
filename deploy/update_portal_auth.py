from pathlib import Path
import shutil
p=Path('/home/zero/portal_site/index.html')
s=p.read_text()
marker='  <!-- Firebase App & Google Auth SDK -->'
if marker not in s:
    assert 'portal-auth.js' in s, 'Unknown portal auth layout'
    print('Portal already uses shared auth')
else:
    start=s.index(marker)
    end=s.index('  </script>',start)+len('  </script>')
    assert 'signInWithPopup' in s[start:end]
    backup=Path('/home/zero/atlantis-auth/index.before-shared-auth-20261111.html')
    assert not backup.exists(), 'Preserve existing rollback copy'
    shutil.copy2(p,backup)
    updated=s[:start]+"  <!-- Shared verified Google / Home Node account flow -->\n  <script src='platform-auth.js?v=20261111-fedcm'></script>\n  <script defer src='portal-auth.js?v=20261111-fedcm'></script>"+s[end:]
    temp=p.with_name('index.auth-new.html')
    temp.write_text(updated)
    temp.replace(p)
    print('Portal shared auth installed; previous HTML preserved')
