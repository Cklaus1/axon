"""Safely materialize the pinned v0.22 parent beside v0.23 amendments. Does not overwrite a repository."""
from pathlib import Path
import argparse, zipfile, shutil
ROOT=Path(__file__).resolve().parents[1]
p=argparse.ArgumentParser();p.add_argument('--dest',type=Path,required=True);a=p.parse_args()
if a.dest.exists() and any(a.dest.iterdir()): raise SystemExit('destination must be absent or empty')
a.dest.mkdir(parents=True,exist_ok=True)
with zipfile.ZipFile(ROOT/'parent/Axon_Cortex_Build_Files_v0_22.zip') as z:
 for n in z.namelist():
  q=Path(n)
  if q.is_absolute() or '..' in q.parts: raise SystemExit('unsafe zip member')
 z.extractall(a.dest/'parent-v0_22')
shutil.copytree(ROOT/'amendments',a.dest/'v0_23-amendments')
shutil.copytree(ROOT/'build',a.dest/'v0_23-build')
print(a.dest)
