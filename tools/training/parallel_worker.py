#!/usr/bin/env python3
"""Launch synchronized PPO learners in the supervised worker's process group."""
import argparse
from contextlib import ExitStack
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time


def launch(processes,worker,args):
    if not 2<=processes<=64:raise ValueError('Choose 2–64 learner processes')
    root=Path(args[args.index('--run-dir')+1]).resolve();owned=root/'.parallel';owned.mkdir(parents=True,exist_ok=False)
    children=[];stopping=False
    def stop(signum,frame):
        nonlocal stopping
        stopping=True
    signal.signal(signal.SIGTERM,stop);signal.signal(signal.SIGINT,stop)
    with ExitStack() as resources:
        try:
            for rank in range(processes):
                env=os.environ.copy();env.update(NEXUS_PARALLEL_ROOT=str(root),WORLD_SIZE=str(processes),RANK=str(rank),LOCAL_RANK=str(rank))
                log=resources.enter_context((owned/f'rank-{rank}.log').open('xb'))
                child=subprocess.Popen([sys.executable,str(worker),*args],env=env,stdin=subprocess.DEVNULL,
                    stdout=None if rank==0 else log,stderr=log)
                children.append(child)
            (owned/'processes.json').write_text(json.dumps(dict(coordinator_pid=os.getpid(),workers=[p.pid for p in children])))
            while True:
                codes=[p.poll() for p in children]
                if stopping:raise RuntimeError('Parallel training stopped')
                failures=[(i,c) for i,c in enumerate(codes) if c not in (None,0)]
                if failures:
                    i,code=failures[0];tail=(owned/f'rank-{i}.log').read_text(errors='replace')[-2500:]
                    raise RuntimeError(f'Learner {i} exited with status {code}: {tail}')
                if all(c==0 for c in codes):return
                time.sleep(.05)
        finally:
            for p in children:
                if p.poll() is None:p.terminate()
            deadline=time.monotonic()+5
            for p in children:
                try:p.wait(timeout=max(.01,deadline-time.monotonic()))
                except subprocess.TimeoutExpired:p.kill();p.wait()


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--processes',type=int,required=True);parser.add_argument('--worker',type=Path,required=True);parser.add_argument('args',nargs=argparse.REMAINDER)
    args=parser.parse_args();args.args=args.args[1:] if args.args[:1]==['--'] else args.args
    try:launch(args.processes,args.worker,args.args)
    except Exception as e:
        print(json.dumps(dict(event='error',message=str(e))),flush=True);raise

if __name__=='__main__':main()
