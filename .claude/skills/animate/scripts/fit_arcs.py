"""Coordinate search on the arm, blade and roll of chosen keys of one clip in tools/gen_heavy_clips.py,
scored frame by frame from the `moves` test (wrist, twist, left hand, locked elbows, tip speed and
height) and the `clearance` test (arms sunk into the trunk). It only nudges: drifting from the written arc costs, so it doesn't shrink the move.

    cargo test --release -p grimora_scene --test moves --test clearance --no-run   # once, builds the exes
    python -I fit_arcs.py <repo> heavy_2 "key(0.35, |key(0.4, |END_2 = dict"

Anchors are the text where a key starts; `ROUNDS` (default 2) sets how many passes. Takes about a
minute per clip. Read the result with `CLIP=<clip> TRACE=4` before keeping it.
"""
import subprocess,sys,re,os,itertools
import glob
repo,clip=sys.argv[1],sys.argv[2]
anchors=sys.argv[3].split("|")
func={"heavy_1":"def write_blow_1","heavy_2":"def write_blow_2","heavy_3":"def write_blow_3"}.get(clip)
def newest(test):
    return max([x for x in glob.glob(repo+f"/target/release/deps/{test}-*") if x.endswith(".exe") or "." not in os.path.basename(x)],key=os.path.getmtime)
exe=newest("moves")
clear=newest("clearance")
p=repo+"/tools/gen_heavy_clips.py"
src=open(p,encoding="utf8").read()
num=r"(-?[\d.]+)"
def region(s):
    if func is None: return 0,len(s)
    a=s.index(func); b=s.index("\ndef ",a+5); return a,b
def get(s,anchor):
    a,b=region(s) if not anchor.endswith("= dict") or anchor.startswith(("coil","crouch","top","hit")) else (0,len(s)); i=s.index(anchor,a)
    am=re.compile(r"arm=\("+num+", "+num+", "+num+r"\)").search(s,i)
    bm=re.compile(r"blade=\("+num+", "+num+r"\)(, roll="+num+")?").search(s,i)
    return [float(am.group(1)),float(am.group(2)),float(am.group(3)),float(bm.group(1)),float(bm.group(2)),float(bm.group(4) or 0)],am,bm
def put(s,anchor,x):
    _,am,bm=get(s,anchor)
    roll=f", roll={x[5]:.1f}" if abs(x[5])>0.05 else ""
    s=s[:bm.start()]+f"blade=({x[3]:.1f}, {x[4]:.1f}){roll}"+s[bm.end():]
    s=s[:am.start()]+f"arm=({x[0]:.1f}, {x[1]:.1f}, {x[2]:.2f})"+s[am.end():]
    return s
def score(s):
    open(p,"w",encoding="utf8",newline="\n").write(s)
    r=subprocess.run(["python","-I",p,clip],cwd=repo,capture_output=True,text=True)
    if r.returncode: return 1e9,"gen error "+r.stderr[-200:]
    o=subprocess.run([exe,"--nocapture"],cwd=repo+"/crates/grimora_scene",env=dict(os.environ,CLIP=clip),capture_output=True,text=True).stdout
    l=[x for x in o.splitlines() if x.startswith("[move] "+clip+" ")][0]
    n=lambda pat: float(re.search(pat,l).group(1))
    short=n(r"short +(\d+)"); ar=n(r"arm +(\d+)"); al=n(r"arm +\d+ +(\d+)")
    w=n(r"wrist +(\d+)"); t=n(r"twist +(\d+)"); lw=n(r"left +(\d+)"); lt=n(r"left +\d+@[\d.]+ +(\d+)")
    low=n(r"tip low +(-?[\d.]+)")
    m=re.search(r"tip [\d.]+/([\d.]+)",l); cut=float(m.group(1)) if m else 0
    tr=subprocess.run([exe,"--nocapture"],cwd=repo+"/crates/grimora_scene",env=dict(os.environ,CLIP=clip,TRACE="2"),capture_output=True,text=True).stdout
    soft=0
    for x in tr.splitlines():
        f=re.match(r"\s+[\d.]+ short +(\d+) foot +\d+ arm +(\d+) +(\d+) wrist +(\d+) twist +(\d+) left +(\d+) +(\d+)",x)
        if f:
            sh,a1,a2,w1,t1,w2,t2=map(int,f.groups())
            soft+=sh*0.5+max(0,w1-50)+max(0,t1-75)+max(0,w2-55)+max(0,t2-75)+3*max(0,a1-95)+3*max(0,a2-97)
    # Forearms and hands under the trunk's skin, from the clearance test, with springs.
    co=subprocess.run([clear,"--nocapture"],cwd=repo+"/crates/grimora_scene",env=dict(os.environ,CLEAR=clip),capture_output=True,text=True).stdout
    deep=0;on=False
    for x in co.splitlines():
        if re.match(r"\[clear\] \S+ springs",x): on="springs true" in x; continue
        m=re.search(r"arm in the trunk: (\d+) mm",x)
        if m and on: deep=max(deep,int(m.group(1)))
    # Amplitude: what the user asked for. Losing it costs more than a joint near its limit.
    reach=re.search(r"arms cover ([\d.]+)/([\d.]+) m, blade +(\d+) deg",o)
    shrink=0
    if reach and func:
        right,left,sweep=float(reach.group(1)),float(reach.group(2)),float(reach.group(3))
        shrink=max(0,0.7-right)*200+max(0,0.62-left)*200+max(0,165-sweep)*2
    pen=soft*0.05+short*2+max(0,deep-15)*3+shrink+max(0,ar-95)*3+max(0,al-97)*3+max(0,w-55)+max(0,t-85)+max(0,lw-58)+max(0,lt-85)+max(0,cut-43)*5+max(0,0.2-low)*300
    return pen,f"deep {deep:3d} "+l[22:150]
s=src; base={a:get(s,a)[0] for a in anchors}
best,info=score(s); print("start",best,info,flush=True)
steps=[(5,[-40,-20,20,40]),(3,[-12,12]),(4,[-12,12]),(0,[-8,8]),(1,[-8,8]),(5,[-8,8]),(3,[-5,5]),(4,[-5,5])]
try:
  for rnd in range(int(os.environ.get("ROUNDS","2"))):
    for a in anchors:
      for i,ds in steps:
        x0=get(s,a)[0]
        for d in ds:
            x=list(x0); x[i]+=d
            drift=sum(abs(x[k]-base[a][k]) for k in (0,1,3,4))*0.12
            s2=put(s,a,x); sc,inf=score(s2)
            if sc+drift < best-0.5:
                best,s,info=sc+drift,s2,inf; print(rnd,a,i,d,round(best,1),inf,flush=True)
finally:
  open(p,"w",encoding="utf8",newline="\n").write(s)
  subprocess.run(["python","-I",p],cwd=repo,capture_output=True)
print("end",best,info)
