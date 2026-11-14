#!/usr/bin/env python3
"""Export an immutable, normalized ONNX policy bundle and verify its numerics."""
import argparse
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def export_duck(robot, checkpoint, target, root):
    import torch
    if robot == "Microduck":
        from microduck_worker import check_source, TASK, COMMIT
        repo = root / "training-stack/microduck_rl"
        check_source(repo)
        import mjlab_microduck.tasks  # noqa: F401
        from mjlab_microduck.export import ExportConfig, run_export
        from rsl_rl.runners import OnPolicyRunner
        saved=torch.load(checkpoint,map_location='cpu',weights_only=True)
        info=saved.get('infos') or {};task=info.get('task_config')
        from contextlib import nullcontext
        context=nullcontext()
        dimensions=61
        if task:
            from authored_microduck import export_configuration
            from duck_task_config import validate,contract,obs_size
            validate(task)
            if task['kind']!='microduck' or info.get('contract')!=contract(task):raise ValueError('Invalid captured Microduck contract')
            candidates=[checkpoint.parent/'model-source.mjz',checkpoint.parent.parent/'model-source.mjz']
            snapshot=next((p for p in candidates if p.is_file() and sha(p)==info.get('source_model_sha256')),None)
            if snapshot is None:raise ValueError('Microduck export requires its captured model-source.mjz')
            context=export_configuration(task,snapshot);dimensions=obs_size(task)
        original = OnPolicyRunner.get_inference_policy
        captured = {}

        def capture(runner, device=None):
            policy = original(runner, device=device)
            captured["model"] = policy.as_jit().cpu().eval()
            return policy

        OnPolicyRunner.get_inference_policy = capture
        try:
            with context:
                run_export(TASK, ExportConfig(checkpoint_file=str(checkpoint),
                                             onnx_file=str(target), num_envs=1, device="cpu"))
        finally:
            OnPolicyRunner.get_inference_policy = original
        return captured["model"], dimensions, 14, COMMIT, "official Microduck exporter with captured configuration" if task else "official Microduck exporter"
    from open_duck_worker import runner_config
    from open_duck_env import ROBOT, COMMIT, OBS_SIZE
    from rsl_rl.models import MLPModel
    from tensordict import TensorDict
    saved = torch.load(checkpoint, map_location="cpu", weights_only=True)
    info = saved.get("infos") or {}
    task=info.get('task_config')
    if task:
        from duck_task_config import validate, obs_size, contract
        validate(task)
        if task['kind']!='open_duck' or info.get('contract')!=contract(task): raise ValueError('Invalid captured Open Duck contract')
        OBS_SIZE=obs_size(task)
    if info.get("robot") != ROBOT or info.get("commit") != COMMIT or info.get("obs_size") != OBS_SIZE:
        raise ValueError("Checkpoint does not match the Open Duck task contract")
    config = copy.deepcopy(runner_config(0, 100, task)["actor"])
    config.pop("class_name")
    actor = MLPModel(TensorDict({"state": torch.zeros(1, OBS_SIZE)}, [1]),
                     {"actor": ["state"]}, "actor", 14, **config)
    actor.load_state_dict(saved["actor_state_dict"], strict=True)
    model = actor.as_jit().cpu().eval()
    torch.onnx.export(model, torch.zeros(1, OBS_SIZE), str(target), dynamo=False,
                      input_names=["observation"], output_names=["action"], opset_version=18)
    return model, OBS_SIZE, 14, COMMIT, "RSL-RL policy with saved observation normalizer"


def export_cpu(checkpoint, target):
    import numpy as np
    import torch
    from torch import nn
    manifest = checkpoint.with_suffix(".txt").read_text()
    shape = next((line.split()[1:] for line in manifest.splitlines() if line.startswith("shape ")), None)
    if not shape or len(shape) != 3:
        raise ValueError("CPU checkpoint has no shape manifest")
    obs, act, hidden = map(int, shape)
    if not 1 <= obs <= 65536 or not 1 <= act <= 4096 or not 1 <= hidden <= 8192:
        raise ValueError("CPU checkpoint shape is invalid")
    weights = np.frombuffer(checkpoint.read_bytes(), dtype="<f4").copy()
    expected = (obs*hidden+hidden + hidden*hidden+hidden)*2 + hidden*act+act + hidden+1 + act
    if weights.size != expected or not np.isfinite(weights).all():
        raise ValueError("CPU checkpoint weights disagree with the manifest")
    layers, index = [], 0
    for i, (n_in, n_out) in enumerate([(obs, hidden), (hidden, hidden), (hidden, act)]):
        layer = nn.Linear(n_in, n_out)
        count = n_in*n_out
        layer.weight.data.copy_(torch.from_numpy(weights[index:index+count].reshape(n_out, n_in)))
        index += count
        layer.bias.data.copy_(torch.from_numpy(weights[index:index+n_out])); index += n_out
        layers.append(layer)
        if i < 2:
            layers.append(nn.Tanh())
    model = nn.Sequential(*layers).eval()
    torch.onnx.export(model, torch.zeros(1, obs), str(target), dynamo=False,
                      input_names=["observation"], output_names=["action"], opset_version=18)
    return model, obs, act, None, "native CPU actor; training uses unnormalized observations"


def verify(model, path, obs, act):
    import numpy as np
    import onnxruntime as ort
    import torch
    session = ort.InferenceSession(str(path), providers=["CPUExecutionProvider"])
    if session.get_inputs()[0].shape[-1] != obs or session.get_outputs()[0].shape[-1] != act:
        raise ValueError("Exported observation/action dimensions changed")
    worst, cases = 0.0, []
    rng = np.random.default_rng(91)
    for scale in (0.0, 0.5, 3.0):
        for _ in range(32):
            sample = rng.normal(0, scale, (1, obs)).astype(np.float32)
            actual = session.run(None, {session.get_inputs()[0].name: sample})[0]
            with torch.inference_mode():
                expected = model(torch.from_numpy(sample)).numpy() if isinstance(model, torch.nn.Module) else model(sample)
            if not np.isfinite(actual).all() or not np.isfinite(expected).all():
                raise ValueError("Policy returned nonfinite actions")
            worst = max(worst, float(np.max(np.abs(actual-expected))))
            cases.append(dict(observation=sample[0].tolist(), source_action=expected[0].tolist(), onnx_action=actual[0].tolist()))
    if worst > 1e-4:
        raise ValueError(f"ONNX differs from source policy: {worst}")
    path.with_name("parity.json").write_text(json.dumps(dict(tolerance=1e-4, cases=cases)))
    return worst


def export(args):
    import torch
    torch.set_num_threads(2)
    checkpoint = args.checkpoint.resolve(strict=True)
    destination = args.output.absolute()
    if destination.exists() or destination.is_symlink():
        raise FileExistsError("Choose a new export directory; existing bundles are preserved")
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.mkdir(exist_ok=False)  # reserve the name without replacing another bundle
    stage = Path(tempfile.mkdtemp(prefix=".nexus-export-", dir=destination.parent))
    try:
        source = stage / ("source" + (".safetensors" if ".safetensors" in checkpoint.name else checkpoint.suffix))
        source.write_bytes(checkpoint.read_bytes())
        source_hash = sha(source)
        if checkpoint.suffix == ".bin":
            shutil.copyfile(checkpoint.with_suffix(".txt"), source.with_suffix(".txt"))
        target = stage / "policy.onnx"
        if source.suffix == ".safetensors":
            if args.robot != 'Unitree G1-29dof': raise ValueError('Native G1 checkpoint requires its G1 robot contract')
            script = args.studio_root / "zealot-stack/zealot/scripts/export_policy_onnx.py"
            subprocess.run([sys.executable, str(script), str(source), str(target)], check=True)
            import onnxruntime as ort
            session = ort.InferenceSession(str(target), providers=["CPUExecutionProvider"])
            obs, act = session.get_inputs()[0].shape[-1], session.get_outputs()[0].shape[-1]
            spec = importlib.util.spec_from_file_location("nexus_zealot_export", script)
            module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
            weights = module.load_safetensors(source)
            maximum = verify(lambda sample: module.numpy_reference(weights, sample[0])[None], target, obs, act)
            commit, exporter = None, "Zealot exporter with source-reference parity checks"
            from g1_env import capture
            capture(stage, args.studio_root, obs, act)
        elif source.suffix == ".bin":
            model, obs, act, commit, exporter = export_cpu(source, target)
            maximum = verify(model, target, obs, act)
        elif source.suffix == ".pt":
            saved = torch.load(source, map_location="cpu", weights_only=True)
            info = saved.get("infos") or {}
            if info.get("task_config", {}).get("kind") == "generic_mujoco":
                if info.get("robot") != args.robot:
                    raise ValueError("Checkpoint robot differs from selected export contract")
                from generic_worker import checkpoint_policy
                model, obs, act, commit, exporter = checkpoint_policy(source, target)
                (stage / "task.json").write_text(json.dumps(info["task_config"], indent=2))
                candidates = [checkpoint.parent / "model.mjz", checkpoint.parent.parent / "model.mjz"]
                snapshot = next((p for p in candidates if p.is_file() and sha(p) == info.get("model_sha256")), None)
                if snapshot is None:
                    raise ValueError("Generic policy export requires its matching captured model.mjz")
                shutil.copyfile(snapshot, stage / "model.mjz")
            else:
                if info.get('task_config',{}).get('kind')=='microduck':
                    candidates=[checkpoint.parent/'model-source.mjz',checkpoint.parent.parent/'model-source.mjz']
                    snapshot=next((p for p in candidates if p.is_file() and sha(p)==info.get('source_model_sha256')),None)
                    if snapshot is None:raise ValueError('Microduck export requires its captured model-source.mjz')
                    shutil.copyfile(snapshot,stage/'model-source.mjz')
                model, obs, act, commit, exporter = export_duck(args.robot, source, target, args.studio_root)
                if info.get('task_config',{}).get('kind') in ('open_duck','microduck'):
                    (stage/'task.json').write_text(json.dumps(info['task_config'],indent=2))
                    candidates=[checkpoint.parent/'model.mjb',checkpoint.parent.parent/'model.mjb']
                    snapshot=next((p for p in candidates if p.is_file() and sha(p)==info.get('model_sha256')),None)
                    if snapshot is None: raise ValueError(f'{args.robot} export requires the matching captured model.mjb')
                    shutil.copyfile(snapshot,stage/'model.mjb')
            maximum = verify(model, target, obs, act)
        else:
            raise ValueError("No ONNX exporter is registered for this robot/checkpoint format")
        import onnx
        model = onnx.load(str(target))
        props = {p.key: p.value for p in model.metadata_props}
        onnx.helper.set_model_props(model, dict(props, **{"nexus.source_checkpoint": source.name, "nexus.source_sha256": source_hash}))
        onnx.save_model(model, str(target), save_as_external_data=False)
        onnx.checker.check_model(str(target))
        if any(t.data_location != 0 for t in onnx.load(str(target), load_external_data=False).graph.initializer):
            raise ValueError("Export still references external tensor files")
        if sha(checkpoint) != source_hash:
            raise ValueError("Source checkpoint changed during export; choose an immutable periodic checkpoint")
        evidence = dict(version=1, robot=args.robot, checkpoint=str(checkpoint), checkpoint_sha256=source_hash,
                        onnx_sha256=sha(target), observations=obs, actions=act, upstream_commit=commit,
                        exporter=exporter, maximum_action_error=maximum, self_contained=True,
                        hardware_validated=False)
        if args.recipe_file:
            json.loads(args.recipe_file.read_text())  # fail malformed recipe copies
            shutil.copyfile(args.recipe_file, stage / "recipe.json")
            evidence["recipe_sha256"] = sha(stage / "recipe.json")
        (stage / "provenance.json").write_text(json.dumps(evidence, indent=2))
        (stage / "README.md").write_text(f"# {args.robot} policy export\n\n"
            f"ONNX accepts {obs} assembled observations and returns {act} actions. "
            "Observation normalization is included where the training backend uses it. "
            "The robot controller must still implement the training task's observation order, "
            "timing, action mapping, actuator limits and safety controls. This export is not a hardware validation.\n")
        # Atomic publication; the worker never exposes a partial successful bundle.
        if not destination.is_dir() or destination.is_symlink() or any(destination.iterdir()):
            raise FileExistsError("Export destination changed while exporting; preserved")
        os.rename(stage, destination)
        print(json.dumps(dict(status="exported", directory=str(destination), **evidence)), flush=True)
    except BaseException:
        shutil.rmtree(stage, ignore_errors=True)
        try:
            destination.rmdir()  # only our empty reservation; preserve unexpected files
        except OSError:
            pass
        raise


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--robot", required=True)
    parser.add_argument("--checkpoint", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--recipe-file", type=Path)
    parser.add_argument("--studio-root", type=Path, default=Path(__file__).resolve().parents[2])
    export(parser.parse_args())
