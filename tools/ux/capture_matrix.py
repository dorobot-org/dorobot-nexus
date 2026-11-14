#!/usr/bin/env python3
"""Capture original pixels after real navigation input in an isolated test app.

Launch with MAKEPAD_HIDE_WINDOWS=1 and DOROBOT_UX_REPORT=<report>, then pass
that process's PID. Never point this driver at someone's working window.
Reports/captures may contain local data: keep output outside the repository.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import time

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('--app', choices=['studio', 'nexus'], required=True)
p.add_argument('--pid', required=True)
p.add_argument('--remote', required=True)
p.add_argument('--report', type=Path, required=True)
p.add_argument('--out', type=Path, required=True)
p.add_argument('--sizes', default='1440x900,1024x768')
p.add_argument('--appearances', default='light,dark')
a = p.parse_args()
a.out.mkdir(parents=True, exist_ok=True)

def command(*words):
    r = subprocess.run([a.remote, 'app', a.pid, *words], text=True, capture_output=True, check=True)
    for line in r.stdout.splitlines():
        value = json.loads(line)
        if 'err' in value or value.get('verified') is False:
            raise RuntimeError(value)
    return r.stdout

def report():
    return json.loads(a.report.read_text())

def wait_for(predicate, timeout=10):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        value = report()
        if predicate(value): return value
        time.sleep(.1)
    raise AssertionError('UI state did not settle')

def click(name):
    item = report()['controls'][name]
    x, y, w, h = item['rect']
    assert item['visible'] and w > 0 and h > 0, name
    command('click', f'x={x+w/2}', f'y={y+h/2}')

def appearance(want):
    def matches(r):
        return (r['state']['dark'] if a.app == 'studio' else r['state']['light'] < .5) == (want == 'dark')
    for _ in range(4):
        if matches(report()): return
        click('appearance' if a.app == 'studio' else 'theme_btn')
        time.sleep(.6)
    raise AssertionError('Appearance did not change')

screens = ([('Library','nav_library'), ('Record','nav_record'), ('Play','nav_play'), ('Hardware','nav_hardware'), ('Eval','nav_eval')]
           if a.app == 'studio' else [(n, f'tab{i}') for i,n in enumerate(['Home','Scenes','Robots','Train','Inspect','Validate','Runs','Deploy'])])
state_key = 'screen' if a.app == 'studio' else 'mode'
receipts = []
for size in a.sizes.split(','):
    width, height = map(int, size.split('x'))
    command('window', 'k=resize', f'width={width}', f'height={height}')
    time.sleep(.8)
    for theme in a.appearances.split(','):
        appearance(theme)
        for screen, control in screens:
            click(control)
            r = wait_for(lambda r: r['state'][state_key] == screen)
            time.sleep(.6)
            stem = f'{a.app}-{size}-{theme}-{screen.lower()}'
            image = a.out/(stem+'.png')
            capture = command('grab', f'out={image.resolve()}')
            r = report()
            (a.out/(stem+'.json')).write_text(json.dumps(r, indent=2)+'\n')
            receipts.append({'file': image.name, 'sha256': hashlib.sha256(image.read_bytes()).hexdigest(),
                             'screen': screen, 'size': [width,height], 'appearance': theme,
                             'input': 'pointer click', 'capture': json.loads(capture.splitlines()[0])})
            print(stem, flush=True)
(a.out/(a.app+'-receipts.json')).write_text(json.dumps(receipts, indent=2)+'\n')
