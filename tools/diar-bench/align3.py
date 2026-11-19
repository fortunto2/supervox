import json,re,datetime as dt,difflib
d=json.load(open('out/asr_full.json')); W=d['wordTimings']
norm=lambda s: re.sub(r'[^\w]','',s.lower())
tok=[norm(w['word']) for w in W]
rows=[json.loads(l) for l in open('/Users/rustam/.supervox/live/sessions/20261119-205946/stream.jsonl')]
t0=dt.datetime.fromisoformat(rows[0]['ts'])
out=[]
for r in rows:
    if r['type']!='transcript': continue
    em=(dt.datetime.fromisoformat(r['ts'])-t0).total_seconds()
    q=[norm(x) for x in r['text'].split() if norm(x)]
    if len(q)<3: out.append(dict(em=em,spk=r['speaker'],text=r['text'],m=None));continue
    best=None
    for i,w in enumerate(W):
        if not (em-200<w['startTime']<em-60): continue
        for L in (len(q),len(q)+2,max(3,len(q)-2)):
            seg=tok[i:i+L]
            sc=difflib.SequenceMatcher(None,q,seg).ratio()
            if best is None or sc>best[0]: best=(sc,i,min(i+L,len(W))-1)
    if best and best[0]>=0.4:
        out.append(dict(em=em,spk=r['speaker'],text=r['text'],m=[W[best[1]]['startTime'],W[best[2]]['endTime'],round(best[0],2)]))
    else: out.append(dict(em=em,spk=r['speaker'],text=r['text'],m=None))
json.dump(out,open('out/aligned.json','w'),ensure_ascii=False,indent=0)
print(sum(1 for o in out if o['m']),len(out))
for o in out:
    m=o['m']; print(f"{o['em']:7.1f} {o['spk']} {('%.1f-%.1f %.2f'%tuple(m)) if m else '-':22} {o['text'][:60]}")
