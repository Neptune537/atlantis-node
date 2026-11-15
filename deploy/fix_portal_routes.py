from pathlib import Path
import shutil
root=Path('/home/zero/portal_site')
p=root/'index.html'
s=p.read_text().replace('href="/game.html"','href="https://heavenlysent-680ac.web.app/?venue=atlantis"')
s=s.replace('portal-auth.js?v=20261111-fedcm','portal-auth.js?v=20261111-lite')
temp=root/'index.routes-new.html'
temp.write_text(s)
temp.replace(p)
game=root/'game.html'
backup=Path('/home/zero/atlantis-auth/game.before-current-route-20261111.html')
if game.exists() and not backup.exists(): shutil.copy2(game,backup)
shutil.copy2(root/'game-redirect.staged.html', game)
print('Portal sign-in entry and game launch now route to current clients')
