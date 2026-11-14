#!/usr/bin/env python3
"""Exercise UI behavior through real clicks/keys; observe the opt-in read-only report."""
import argparse,json,subprocess,time
from pathlib import Path
p=argparse.ArgumentParser(description=__doc__)
p.add_argument('--app',choices=['studio','nexus'],required=True)
p.add_argument('--pid',required=True);p.add_argument('--remote',required=True)
p.add_argument('--report',type=Path,required=True);p.add_argument('--out',type=Path,required=True)
a=p.parse_args();a.out.mkdir(parents=True,exist_ok=True);checks=[]
def cmd(*args):
 r=subprocess.run([a.remote,'app',a.pid,*args],text=True,capture_output=True,check=True)
 for line in r.stdout.splitlines():
  j=json.loads(line)
  if 'err' in j or j.get('verified') is False:raise RuntimeError(j)
 return r.stdout

def read():return json.loads(a.report.read_text())
def wait(test):
 end=time.monotonic()+10
 while time.monotonic()<end:
  r=read()
  if test(r):return r
  time.sleep(.1)
 raise AssertionError('UI assertion timed out')
def click(name):
 r=read()['controls'][name];x,y,w,h=r['rect'];assert r['visible'] and w>0 and h>0,name
 cmd('click',f'x={x+w/2}',f'y={y+h/2}');time.sleep(.35)
def key(code,**mod):cmd('key',f'c={code}',*[f'{k}={v}' for k,v in mod.items()]);time.sleep(.25)
def check(name,condition):
 assert condition,name
 checks.append({'name':name,'pass':True,'state':read()['state']});print(name,flush=True)
def shot(name):cmd('grab',f'out={(a.out/(name+".png")).resolve()}')
if a.app=='studio':
 click('nav_play');wait(lambda r:r['state']['screen']=='Play')
 if read()['controls']['page_play.upper.side']['visible']:click('details_btn')
 if not read()['controls']['page_play.upper.tree']['visible']:click('episodes_btn')
 if read()['state'].get('signals_visible'):click('signals_btn')
 click('page_play.upper.tree.tree_body.row_1');wait(lambda r:r['state']['episode']==1)
 check('select second Nano episode',read()['state']['frames']==360)
 click('play_btn');wait(lambda r:r['state']['playing'] and r['state']['time']>.1)
 click('play_btn');paused=wait(lambda r:not r['state']['playing'])['state']['time']
 key('ArrowRight');check('pause and single-frame step',abs(read()['state']['time']-paused-1/30)<.005)
 key('Home');check('seek to start',read()['state']['time']==0)
 timeline=read()['controls']['page_play.timeline.tl_body.tl']['rect']
 x,y,w,h=timeline;cmd('click',f'x={x+w*.25}',f'y={y+h*.5}')
 wait(lambda r:r['state']['time']>1);position=read()['state']['time'];check('pointer seek',1<position<5)
 click('nav_library');click('nav_play')
 check('navigation retains episode and position',read()['state']['episode']==1 and abs(read()['state']['time']-position)<.005)
 click('details_btn');click('signals_btn');dark=read()['state']['dark'];click('appearance')
 wait(lambda r:r['state']['dark']!=dark)
 check('appearance retains current task and details',read()['state']['episode']==1 and abs(read()['state']['time']-position)<.005 and read()['controls']['page_play.upper.side']['visible'])
 if read()['state']['tag']=='Some(Good)':click('page_play.upper.side.side_body.btn_good')
 click('page_play.upper.side.side_body.btn_good');wait(lambda r:r['state']['tag']=='Some(Good)')
 check('tag episode through UI',read()['state']['tag']=='Some(Good)');shot('replay-details-and-tag')
 cmd('mouse','k=scroll','x=260','y=690','dx=0','dy=120');time.sleep(.4)
 for _ in range(5):click('signal_next')
 check('last signal is reachable',read()['controls']['page_play.timeline.tl_body.lanes.content.lane_5']['visible'] and not read()['controls']['page_play.timeline.tl_body.lanes.content.lane_0']['visible']);shot('last-signal')
 for _ in range(5):click('signal_prev')
 click('nav_library');click('page_library.left_col.header.btn_pull');wait(lambda r:r['state']['hub_open'])
 click('hub_repo');cmd('key','t=sample/preview');key('Escape');wait(lambda r:not r['state']['hub_open'])
 check('cancel transfer form retains episode',read()['state']['episode']==1)
 click('nav_record');check('unconfigured capture has a setup path',read()['controls']['setup_page']['visible'])
 click('setup_rescan');check('hardware check returns without recording',not read()['state']['playing'])
 click('setup_replay');wait(lambda r:r['state']['screen']=='Play')
 check('return from setup to replay',read()['state']['episode']==1)
else:
 click('tab2');wait(lambda r:r['state']['mode']=='Robots');robot=read()['state']['robot']
 if read()['state']['inspector_open']:click('inspector_btn')
 check('Nano opened in writable workspace',bool(robot) and read()['state']['workspace_writable'] and read()['state']['robot_structure']==[27,26,17,134])
 click('inspector_btn');check('compact details replaces list',read()['controls']['rail_r']['visible'] and not read()['controls']['rail_l']['visible']);shot('nano-details')
 click('help_btn');wait(lambda r:r['state']['modal']!='None');shot('help-dialog');key('Escape')
 wait(lambda r:r['state']['modal']=='None');check('Escape dismisses dialog without changing robot',read()['state']['robot']==robot)
 click('inspector_btn');click('tab1');click('filter_in');cmd('key','t=does-not-match-any-scene')
 wait(lambda r:r['state']['filter']=='does-not-match-any-scene');shot('search-empty')
 key('KeyA',logo=1);key('Backspace');wait(lambda r:r['state']['filter']=='')
 check('search and clear preserve selection',read()['state']['robot']==robot)
 check('Select All does not animate the robot',not read()['state']['robot_animating'])
 click('b_rename');wait(lambda r:r['state']['modal']!='None');click('modal_host.panel.foot.b_cancel');wait(lambda r:r['state']['modal']=='None')
 check('cancel rename dialog',read()['state']['robot']==robot)
 theme=read()['state']['theme'];click('theme_btn');wait(lambda r:r['state']['theme']!=theme)
 check('appearance preserves selected robot',read()['state']['robot']==robot)
 for i,mode in enumerate(['Home','Scenes','Robots','Train','Inspect','Validate','Runs','Deploy']):
  key(f'Key{i+1}',logo=1);wait(lambda r:r['state']['mode']==mode)
 check('all workspace keyboard shortcuts retain robot',read()['state']['robot']==robot)
 click('tab2');check('workspace remains saved',read()['state']['workspace_error'] is None)
(a.out/'journeys.json').write_text(json.dumps(checks,indent=2)+'\n')
