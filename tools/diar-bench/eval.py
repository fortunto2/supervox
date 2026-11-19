import json,re,wave,sys,collections
import numpy as np
SR=16000
def read(p):
    w=wave.open(p); return np.frombuffer(w.readframes(w.getnframes()),np.int16).astype(np.float32)/32768
import onnxruntime as ort, kaldi_native_fbank as knf
sess=ort.InferenceSession('/Users/rustam/.supervox/models/wespeaker_en_voxceleb_CAM++.onnx',providers=['CPUExecutionProvider'])
def emb(x):
    o=knf.FbankOptions(); o.frame_opts.dither=0; o.frame_opts.snip_edges=True; o.frame_opts.samp_freq=SR; o.mel_opts.num_bins=80
    f=knf.OnlineFbank(o); f.accept_waveform(SR,x.tolist()); f.input_finished()
    F=np.stack([f.get_frame(i) for i in range(f.num_frames_ready)]); F=(F-F.mean(0))[None].astype(np.float32)
    e=sess.run(None,{sess.get_inputs()[0].name:F})[0][0]; return e/np.linalg.norm(e)
vx=read('/Users/rustam/.supervox/voices/Рустам.wav')
import subprocess
# voiceprint wav may be 48k: resample via ffmpeg
subprocess.run(['ffmpeg','-nostdin','-loglevel','error','-y','-i','/Users/rustam/.supervox/voices/Рустам.wav','-ac','1','-ar','16000','-c:a','pcm_s16le','vp16.wav'],check=True)
vp=read('vp16.wav'); vpe=np.mean([emb(vp[i:i+160000]) for i in range(0,min(len(vp),480000),160000) if len(vp[i:i+160000])>8000],0); vpe/=np.linalg.norm(vpe)
x=read('wav/r175952.wav')
# windows
win=[]
for l in open('out/sim_all.txt'):
    m=re.match(r't=\s*([\d.]+)s rms=([\d.]+) raw=(-?[\d.]+)',l)
    if m: win.append((float(m[1]),float(m[1])+4,float(m[2]),float(m[3])))
R=[w for w in win if w[3]>=0.60]
N=[w for w in win if w[3]<=0.20 and w[2]>0.005]
print('windows R',len(R),'nonR speech',len(N))
def load(name):
    if name=='offline':
        d=json.load(open('out/off175952.json')); return [(s['speakerId'],s['startTimeSeconds'],s['endTimeSeconds']) for s in d['segments']]
    if name=='sortformer':
        d=json.load(open('out/sf175952.json')); return [(s['speaker'],s['startTimeSeconds'],s['endTimeSeconds']) for s in d['segments']]
    if name=='live':
        a=json.load(open('out/aligned.json')); return [(o['spk'],o['m'][0],o['m'][1]) for o in a if o['m']]
def at(segs,a,b):
    cov=collections.Counter()
    for s,st,en in segs:
        sh=min(b,en)-max(a,st)
        if sh>0: cov[s]+=sh
    if not cov: return None
    s,c=cov.most_common(1)[0]
    return s if c>=0.3*(b-a) else None
def owner_by_voice(segs):
    best=None; per={}
    spk=sorted({s for s,_,_ in segs})
    for s in spk:
        mine=sorted([t for t in segs if t[0]==s and t[2]-t[1]>=1.0],key=lambda t:t[1]-t[2])
        audio=[];tot=0
        for _,a,b in mine:
            audio.append(x[int(a*SR):int(b*SR)]); tot+=b-a
            if tot>=30: break
        if tot<1: continue
        a=np.concatenate(audio); ch=[a[i:i+160000] for i in range(0,len(a),160000) if len(a[i:i+160000])>8000]
        e=np.mean([emb(c) for c in ch],0); e/=np.linalg.norm(e); per[s]=float(e@vpe)
    o=max(per,key=per.get) if per else None
    return o,per
def owner_oracle(segs):
    c=collections.Counter()
    for a,b,_,_ in R:
        s=at(segs,a,b)
        if s: c[s]+=1
    return c.most_common(1)[0][0] if c else None
for name in ('live','offline','sortformer'):
    segs=load(name)
    own,per=(owner_oracle(segs),{}) if name=='live' else owner_by_voice(segs)
    nspk=len({s for s,_,_ in segs})
    rr=[at(segs,a,b) for a,b,_,_ in R]; nn=[at(segs,a,b) for a,b,_,_ in N]
    rec=sum(1 for s in rr if s==own)/len(R)
    miss=sum(1 for s in rr if s is None)/len(R)
    fa=sum(1 for s in nn if s==own and own is not None)/len(N)
    err=(sum(1 for s in rr if s!=own)+sum(1 for s in nn if s==own))/(len(R)+len(N))
    spread=len({s for s in rr if s})
    covN=sum(1 for s in nn if s is not None)/len(N)
    print(f"{name:11} speakers={nspk:2d} owner={own} ({'oracle' if name=='live' else 'voiceprint '+str({k:round(v,2) for k,v in sorted(per.items(),key=lambda kv:-kv[1])[:3]})}) recall={rec:.2f} missed(no-speaker)={miss:.2f} false-owner={fa:.3f} binaryErr={err:.3f} owner-in-N-clusters={spread} nonR-coverage={covN:.2f}")

print('--- hand-labeled, voiceprint-agreed segments')
gt=[l.rstrip('\n').split('\t') for l in open('gt_final.tsv')]
for name in ('live','offline','sortformer'):
    segs=load(name)
    own=owner_oracle(segs) if name=='live' else owner_by_voice(segs)[0]
    ok=0
    for i,a,b,lab in gt:
        s=at(segs,float(a),float(b)); pred='R' if (s==own and s) else 'G'
        ok+= pred==lab
    print(name, f"{ok}/{len(gt)}")
print('--- Rustam alone 45 s')
x=read('wav/r193531.wav')
def load2(p,k):
    d=json.load(open(p)); 
    return [(s['speakerId'],s['startTimeSeconds'],s['endTimeSeconds']) for s in d['segments']] if k=='o' else [(s['speaker'],s['startTimeSeconds'],s['endTimeSeconds']) for s in d['segments']]
win=[]
for l in open('/dev/stdin') if False else []: pass
rows=[]
for l in open('out/sim_alone.txt'):
    m=re.match(r't=\s*([\d.]+)s rms=([\d.]+) raw=(-?[\d.]+)',l)
    if m: rows.append((float(m[1]),float(m[1])+4,float(m[2]),float(m[3])))
Rw=[w for w in rows if w[3]>=0.6]
for name,segs in (('offline',load2('out/off193531.json','o')),('sortformer',load2('out/sf193531.json','s'))):
    spk=collections.Counter()
    for a,b,_,_ in Rw:
        s=at(segs,a,b); spk[s]+=1
    own,per=owner_by_voice(segs)
    print(name,'speakers',len({s for s,_,_ in segs}),'owner',own,{k:round(v,2) for k,v in per.items()},'R-windows by speaker',dict(spk),'of',len(Rw))
