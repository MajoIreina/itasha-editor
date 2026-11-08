"""Fetch discovered public resources, streaming large .mi files to avoid CDP cache loss."""
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor,as_completed
from urllib.request import Request,urlopen
from urllib.parse import urlsplit
import json,hashlib,shutil,datetime,re
ROOT=Path(__file__).resolve().parents[1];VEHICLES=ROOT/'vehicles';POOL=VEHICLES/'data'
old=[]
for name in ('manifest.json','extras.json'):
 value=json.loads((ROOT/'resources'/name).read_text(encoding='utf-8'));old+=value.get('entries',[]) if isinstance(value,dict) else value
known={e['url']:e for e in old if e.get('status')==200 and e.get('bytes')}
manifests={p.parent.name:json.loads(p.read_text(encoding='utf-8')) for p in VEHICLES.glob('*/manifest.json')};wanted={}
def walk(value,urls):
 if isinstance(value,str) and re.match(r'https?://',value) and re.search(r'\.(?:png|jpe?g|webp|svg|gif|js|css|wasm|json|mi|bin|ktx2?)(?:\?|$)',value,re.I):urls.add(value)
 elif isinstance(value,dict):
  for v in value.values():walk(v,urls)
 elif isinstance(value,list):
  for v in value:walk(v,urls)
for id,m in manifests.items():
 urls={e['url'] for e in m['failures'] if '.mi' in e['url']}
 for file in ('configuration.json','materials.json','ConfigTable.json'):
  p=VEHICLES/id/file
  if p.exists():walk(json.loads(p.read_text(encoding='utf-8')),urls)
 present={e['url'] for e in m['entries']}
 for url in urls-present:wanted.setdefault(url,[]).append(id)
def download(url):
 if url in known:
  e=known[url].copy();src=ROOT/'resources'/e['file'];dest=VEHICLES/e['file']
  if not dest.exists():shutil.copyfile(src,dest)
  return e
 request=Request(url,headers={'Referer':'https://www.xiaomiev.com/','User-Agent':'Mozilla/5.0'})
 pending=POOL/('download-'+hashlib.sha256(url.encode()).hexdigest()+'.pending')
 for attempt in range(3):
  try:
   with urlopen(request,timeout=90) as r,pending.open('wb') as out:
    digest=hashlib.sha256();size=0
    while block:=r.read(1024*1024):digest.update(block);size+=len(block);out.write(block)
    headers={'content-type':r.headers.get('content-type','application/octet-stream')}
   file='data/'+digest.hexdigest();pending.replace(VEHICLES/file)
   return {'url':url,'method':'GET','postData':None,'status':200,'headers':headers,'file':file,'bytes':size,'type':'asset','savedAt':datetime.datetime.now(datetime.timezone.utc).isoformat()}
  except Exception:
   if attempt==2:raise
print('RESOURCES',len(wanted),flush=True);failures=[]
with ThreadPoolExecutor(max_workers=4) as executor:
 tasks={executor.submit(download,url):url for url in wanted}
 for i,task in enumerate(as_completed(tasks),1):
  url=tasks[task]
  try:
   entry=task.result()
   for id in wanted[url]:manifests[id]['entries'].append(entry)
   if urlsplit(url).path.endswith('.mi') or i%25==0:print('SAVED',i,len(wanted),entry['bytes'],url,flush=True)
  except Exception as exc:failures.append({'url':url,'error':str(exc)});print('FAILED',url,str(exc),flush=True)
  for id in wanted[url]:(VEHICLES/id/'manifest.json').write_text(json.dumps(manifests[id],ensure_ascii=False,indent=2),encoding='utf-8')
(VEHICLES/'download-failures.json').write_text(json.dumps(failures,ensure_ascii=False,indent=2),encoding='utf-8');print('DONE',len(wanted)-len(failures),'failures',len(failures),flush=True)
