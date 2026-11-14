"""Shared-policy PPO across local processes on one training host.

RSL-RL owns gradient averaging and adaptive-KL synchronization. This adapter
also synchronizes observation statistics and verifies identical final state.
"""
import hashlib
import json
import os
from pathlib import Path
from datetime import timedelta
from contextlib import contextmanager
from microduck_motion import atomic_json


def world():return int(os.environ.get('WORLD_SIZE','1')) if os.environ.get('NEXUS_PARALLEL_ROOT') else 1
def rank():return int(os.environ.get('RANK','0')) if world()>1 else 0
def total_steps(local):return int(local)*world()

@contextmanager
def single_process_export():
    """An inference-only exporter must not join the training process group."""
    names=('WORLD_SIZE','RANK','LOCAL_RANK');old={n:os.environ.get(n) for n in names}
    os.environ.update(WORLD_SIZE='1',RANK='0',LOCAL_RANK='0')
    try:yield
    finally:
        for n,v in old.items():
            if v is None:os.environ.pop(n,None)
            else:os.environ[n]=v

def prepare_args(args):
    if world()>1:
        args.seed=(args.seed+rank()*1000003)%(2**32)
        if rank():args.run_dir=Path(os.environ['NEXUS_PARALLEL_ROOT'])/'.parallel'/f'rank-{rank()}'
    return args

def device(requested):
    if world()==1:return requested
    import torch
    if requested=='cpu':return 'cpu'
    if requested=='cuda' and torch.cuda.is_available() and rank()<torch.cuda.device_count():
        torch.cuda.set_device(rank());return f'cuda:{rank()}'
    raise ValueError('Parallel PPO requires CPU or one CUDA device per learner process. Apple MPS uses one learner process; no device fallback was started.')

def emit(stream,event,values):
    if world()>1 and event=='sample':
        import torch.distributed as dist
        samples=[None]*world();dist.all_gather_object(samples,values)
        if rank():return
        values=values.copy();values['step']=sum(s['step'] for s in samples)
        for key in ('reward','episode_len'):
            values[key]=sum(s[key] for s in samples)/world()
        values['steps_per_sec']=sum(s['iteration_env_steps'] for s in samples)/max(s['iteration_seconds'] for s in samples)
        count=sum(s.get('completed_episodes',0) for s in samples)
        values['fall_rate']=sum(s['fall_rate']*s.get('completed_episodes',0) for s in samples)/count if count else 0.
        if count:values['episode_len']=sum(s['episode_len']*s.get('completed_episodes',0) for s in samples)/count
        values['fall_rate_measured']=count>0;values['completed_episodes']=count;values['processes']=world()
        values['metric_scope']='all learner processes; rolling episode outcomes'
    if rank()==0:print(json.dumps(dict(event=event,**values),allow_nan=False),file=stream,flush=True)

def state_hash(value):
    import torch
    h=hashlib.sha256()
    def visit(v):
        if isinstance(v,torch.Tensor):
            h.update(str((v.dtype,tuple(v.shape))).encode());h.update(v.detach().cpu().contiguous().numpy().tobytes())
        elif isinstance(v,dict):
            for k in sorted(v,key=str):h.update(str(k).encode());visit(v[k])
        elif isinstance(v,(tuple,list)):
            for item in v:visit(item)
        else:h.update(json.dumps(v,allow_nan=False).encode())
    visit(value.state_dict() if hasattr(value,'state_dict') else value)
    return h.hexdigest()

class RunnerMixin:
    def _configure_multi_gpu(self):
        import torch
        self.gpu_world_size=world();self.gpu_global_rank=rank();self.gpu_local_rank=rank();self.is_distributed=world()>1
        self.cfg['multi_gpu']=dict(global_rank=rank(),local_rank=rank(),world_size=world()) if self.is_distributed else None
        if self.is_distributed:
            backend='nccl' if str(self.device).startswith('cuda:') else 'gloo'
            if backend=='gloo' and self.device!='cpu':raise ValueError('Gloo PPO requires CPU tensors')
            rendezvous=(Path(os.environ['NEXUS_PARALLEL_ROOT'])/'.parallel/rendezvous').resolve().as_uri()
            torch.distributed.init_process_group(backend=backend,init_method=rendezvous,rank=rank(),world_size=world(),timeout=timedelta(minutes=10))

    def __init__(self,*args,**kwargs):
        super().__init__(*args,**kwargs)
        if self.is_distributed:
            import torch
            # Normalization must use all ranks' observations, otherwise policies
            # have different effective input transforms despite equal weights.
            for model in (self.alg.actor,self.alg.critic):
                normalizer=model.obs_normalizer
                if not hasattr(normalizer,'update'):continue
                original=normalizer.update
                def synchronized(x,update=original):
                    with torch.inference_mode(False),torch.no_grad():
                        batch=x.detach().clone();parts=[torch.empty_like(batch) for _ in range(world())]
                        torch.distributed.all_gather(parts,batch)
                        update(torch.cat(parts,dim=0))
                normalizer.update=synchronized

    def learn(self,*args,**kwargs):
        if not self.is_distributed:return super().learn(*args,**kwargs)
        import torch
        initial=state_hash(self.alg.actor)
        try:
            result=super().learn(*args,**kwargs)
            item=dict(rank=rank(),seed=self.cfg.get('seed'),device=str(self.device),actor_sha256=state_hash(self.alg.actor),
                      critic_sha256=state_hash(self.alg.critic),initial_actor_sha256=initial,
                      optimizer_sha256=state_hash(self.alg.optimizer),
                      observation_samples=int(self.alg.actor.obs_normalizer.count),optimizer_states=len(self.alg.optimizer.state),
                      completed_iterations=self.current_learning_iteration+1)
            results=[None]*world();torch.distributed.all_gather_object(results,item)
            for field in ('actor_sha256','critic_sha256','optimizer_sha256','observation_samples','completed_iterations'):
                if len({r[field] for r in results})!=1:raise RuntimeError('Parallel PPO final states disagree: '+field)
            if rank()==0:
                atomic_json(Path(os.environ['NEXUS_PARALLEL_ROOT'])/'parallel-result.json',dict(version=1,processes=world(),
                    backend=torch.distributed.get_backend(),gradient_aggregation='RSL-RL mean across all ranks',
                    observation_normalization='pooled samples from all ranks',ranks=results))
            return result
        finally:torch.distributed.destroy_process_group()
