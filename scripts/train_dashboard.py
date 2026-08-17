#!/usr/bin/env python3
"""Live dashboard for a biped_train_gpu run.

Parses the trainer's stdout log (the human table rows plus the `[rb]` term
breakdown) into a self-contained auto-refreshing HTML page with charts for
the whole run so far: reward, fall rate, terrain curriculum level, torso
height, KL, iteration time, and the latest reward-term breakdown.

Also snapshots the checkpoint every --snap-iters iterations into
<run_dir>/checkpoints/iter_NNNNN.safetensors, so the policy's evolution can
be replayed after the fact (the trainer itself overwrites its checkpoint).

Usage:
  train_dashboard.py <run_dir> [--total 7000] [--watch] [--snap-iters 250]

<run_dir> must contain train.log and policy.safetensors. Writes
<run_dir>/dashboard.html. With --watch, regenerates every 30 s forever.
Python 3.9-compatible, stdlib only.
"""
import argparse
import json
import os
import re
import shutil
import sys
import time

ROW = re.compile(
    r"^\s*(\d+)\s+([\d.]+)\s+(-?[\d.]+)\s+(\d+)\s+(-?[\d.]+)\s+([\d.eE+-]+)\s+([\d.]+)\s+([\d.]+)\s*$"
)
RB = re.compile(r"^\[rb\] iter (\d+) (.*)$")
KV = re.compile(r"([A-Za-z_0-9]+)=(-?[\d.]+)")


def parse(log_path):
    rows = []          # iter, curr, reward, falls, torso_z, lr, kl, sec
    rb = {}            # iter -> {term: value, ...}
    with open(log_path, "r", errors="replace") as f:
        for line in f:
            m = ROW.match(line)
            if m:
                rows.append([
                    int(m.group(1)), float(m.group(2)), float(m.group(3)),
                    int(m.group(4)), float(m.group(5)), float(m.group(6)),
                    float(m.group(7)), float(m.group(8)),
                ])
                continue
            m = RB.match(line)
            if m:
                rb[int(m.group(1))] = dict(
                    (k, float(v)) for k, v in KV.findall(m.group(2))
                )
    return rows, rb


def series(rows, rb):
    it = [r[0] for r in rows]
    out = {
        "iter": it,
        "reward": [r[2] for r in rows],
        "torso_z": [r[4] for r in rows],
        "kl": [r[6] for r in rows],
        "sec": [r[7] for r in rows],
    }
    fall_rate, terrain = [], []
    for i in it:
        d = rb.get(i, {})
        s = d.get("samples", 0.0)
        fall_rate.append(d.get("term_fell", 0.0) / s if s else 0.0)
        terrain.append(d.get("terrain_level", 0.0))
    out["fall_rate"] = fall_rate
    out["terrain"] = terrain
    return out


def latest_terms(rb):
    if not rb:
        return []
    d = rb[max(rb)]
    skip = {"samples", "terrain_level", "term_fell", "term_illegal", "term_timeout"}
    terms = [(k, v) for k, v in d.items() if k not in skip]
    terms.sort(key=lambda kv: abs(kv[1]), reverse=True)
    return terms[:18]


HTML = """<!doctype html>
<meta charset="utf-8">
<meta http-equiv="refresh" content="30">
<title>G1 rough-slope training</title>
<style>
 body {{ font: 13px -apple-system, sans-serif; background:#0f1420; color:#dfe6f2;
        margin: 24px; }}
 h1 {{ font-size: 17px; margin: 0 0 4px; }}
 .sub {{ color:#8a98ae; margin-bottom: 18px; }}
 .grid {{ display:grid; grid-template-columns: repeat(auto-fill, minmax(430px,1fr));
         gap:16px; }}
 .card {{ background:#171e2e; border:1px solid #26314a; border-radius:8px;
         padding:10px 12px; }}
 .card h2 {{ font-size:12px; color:#9fb0ca; margin:0 0 6px; font-weight:600; }}
 canvas {{ width:100%; height:150px; }}
 .terms td {{ padding:1px 8px 1px 0; font-family: ui-monospace, monospace;
             font-size:11.5px; }}
 .pos {{ color:#5ad7a0 }} .neg {{ color:#ff7a7a }}
 .bar {{ height:8px; background:#26314a; border-radius:4px; overflow:hidden;
        margin:8px 0 16px; }}
 .bar div {{ height:100%; background:#25a5e8 }}
</style>
<h1>G1 · rough slope 8° · zealot GPU training</h1>
<div class="sub">{status}</div>
<div class="bar"><div style="width:{pct}%"></div></div>
<div class="grid">
 <div class="card"><h2>mean step reward</h2><canvas id="reward"></canvas></div>
 <div class="card"><h2>fall rate (per sample)</h2><canvas id="fall_rate"></canvas></div>
 <div class="card"><h2>terrain curriculum level</h2><canvas id="terrain"></canvas></div>
 <div class="card"><h2>torso height (m)</h2><canvas id="torso_z"></canvas></div>
 <div class="card"><h2>policy KL</h2><canvas id="kl"></canvas></div>
 <div class="card"><h2>seconds / iteration</h2><canvas id="sec"></canvas></div>
 <div class="card"><h2>latest reward terms (|largest| first)</h2>
   <table class="terms">{terms}</table></div>
</div>
<script>
const S = {data};
function plot(id, xs, ys) {{
  const c = document.getElementById(id);
  const dpr = window.devicePixelRatio || 1;
  c.width = c.clientWidth * dpr; c.height = c.clientHeight * dpr;
  const g = c.getContext("2d");
  if (!xs.length) return;
  let lo = Math.min(...ys), hi = Math.max(...ys);
  if (hi - lo < 1e-9) {{ hi = lo + 1; }}
  const px = i => (xs[i] - xs[0]) / Math.max(1, xs[xs.length-1] - xs[0]) * (c.width - 8) + 4;
  const py = v => c.height - 4 - (v - lo) / (hi - lo) * (c.height - 8);
  g.strokeStyle = "#26314a"; g.beginPath();
  g.moveTo(0, py(0)); g.lineTo(c.width, py(0)); g.stroke();
  g.strokeStyle = "#25a5e8"; g.lineWidth = 1.5 * dpr; g.beginPath();
  for (let i = 0; i < xs.length; i++) (i ? g.lineTo(px(i), py(S[id][i])) : g.moveTo(px(i), py(S[id][i])));
  g.stroke();
  g.fillStyle = "#8a98ae"; g.font = `${{10*dpr}}px monospace`;
  g.fillText(hi.toFixed(3), 4, 10 * dpr);
  g.fillText(lo.toFixed(3), 4, c.height - 4);
  g.fillText(S[id][S[id].length-1].toFixed(3), c.width - 60 * dpr, 10 * dpr);
}}
for (const k of ["reward","fall_rate","terrain","torso_z","kl","sec"]) plot(k, S.iter, S[k]);
</script>
"""


def render(run_dir, total):
    log = os.path.join(run_dir, "train.log")
    rows, rb = parse(log)
    data = series(rows, rb)
    cur = rows[-1][0] if rows else 0
    pct = min(100.0, 100.0 * cur / total) if total else 0.0
    if rows:
        recent = rows[-20:]
        sec = sum(r[7] for r in recent) / len(recent)
        eta_h = (total - cur) * sec / 3600.0
        d = rb.get(cur, {})
        sps = d.get("samples", 0.0) / sec if sec else 0.0
        status = (
            "iter {}/{} · {:.1f}%% · {:.1f}s/iter · {:.0f} samples/s · ETA {:.1f} h"
            .replace("%%", "%")
        ).format(cur, total, pct, sec, sps, eta_h)
    else:
        status = "waiting for first iteration…"
    terms = "".join(
        '<tr><td>{}</td><td class="{}">{:+.5f}</td></tr>'.format(
            k, "pos" if v >= 0 else "neg", v
        )
        for k, v in latest_terms(rb)
    )
    html = HTML.format(
        status=status, pct="{:.1f}".format(pct),
        terms=terms or "<tr><td>—</td></tr>",
        data=json.dumps({k: [round(x, 5) for x in v] for k, v in data.items()}),
    )
    tmp = os.path.join(run_dir, ".dashboard.tmp")
    with open(tmp, "w") as f:
        f.write(html)
    os.replace(tmp, os.path.join(run_dir, "dashboard.html"))
    return cur


def snapshot(run_dir, cur, snap_iters, state):
    """Copy the live checkpoint every `snap_iters` iterations."""
    if cur - state.get("last", -10**9) < snap_iters:
        return
    src = os.path.join(run_dir, "policy.safetensors")
    if not os.path.exists(src):
        return
    dst_dir = os.path.join(run_dir, "checkpoints")
    os.makedirs(dst_dir, exist_ok=True)
    dst = os.path.join(dst_dir, "iter_{:05d}.safetensors".format(cur))
    if not os.path.exists(dst):
        shutil.copy2(src, dst)
    state["last"] = cur


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("run_dir")
    ap.add_argument("--total", type=int, default=7000)
    ap.add_argument("--watch", action="store_true")
    ap.add_argument("--snap-iters", type=int, default=250)
    a = ap.parse_args()

    state = {}
    while True:
        try:
            cur = render(a.run_dir, a.total)
            snapshot(a.run_dir, cur, a.snap_iters, state)
        except Exception as e:
            print("dashboard error:", e, file=sys.stderr)
        if not a.watch:
            break
        time.sleep(30)


if __name__ == "__main__":
    main()
