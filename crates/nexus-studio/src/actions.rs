//! Every user action from the mockup's ACT dispatcher, as Store methods.
//! Gate/ping timers are modeled as tick-advanced progress so the whole
//! machine stays synchronous and testable; the app drives `fast_tick`
//! (~120ms) and `train_tick` (~1.1s).

use crate::i18n::trf;
use crate::state::*;

fn f2(v: f64) -> String {
    format!("{v:.2}")
}

impl Store {
    pub fn tr_pub(&self, k: &str) -> String {
        crate::i18n::tr(self.lang, k).to_string()
    }
    fn tr(&self, k: &str) -> String {
        crate::i18n::tr(self.lang, k).to_string()
    }
    fn trf(&self, k: &str, a: &[&str]) -> String {
        trf(self.lang, k, a)
    }

    // ================================================================ scenes

    pub fn sel_scene(&mut self, id: &str) {
        self.draft = None;
        self.dirty = false;
        self.sel.scene = Some(id.into());
        if let Some(st) = self.sets_of(id).first() {
            self.sel.set = Some(st.id.clone());
        }
    }

    pub fn new_scene(&mut self) {
        let id = self.nid("s");
        let name = format!("scene-{}", self.uid - 100);
        let s = Scene::base(&id, &name);
        self.scenes.push(s);
        self.sel.scene = Some(id);
        self.draft = None;
        self.dirty = false;
        let m = self.tr("New scene from baseline — Draft until saved");
        self.toast(m);
    }

    pub fn dup_scene(&mut self) {
        let Some(src) = self.sel.scene.clone().and_then(|i| self.scene(&i).cloned()) else { return };
        let id = self.nid("s");
        let mut s = src.clone();
        s.id = id.clone();
        s.name = format!("{}-2", src.name);
        let nm = s.name.clone();
        self.scenes.push(s);
        self.sel.scene = Some(id);
        let m = self.trf("Duplicated as {0}", &[&nm]);
        self.toast(m);
    }

    pub fn del_scene(&mut self) {
        if self.scenes.len() <= 1 {
            self.modal = AppModal::LastSceneBlocked;
            return;
        }
        self.modal = AppModal::DeleteScene;
    }

    pub fn del_scene_yes(&mut self) {
        let Some(id) = self.sel.scene.clone() else { return };
        let Some(idx) = self.scenes.iter().position(|s| s.id == id) else { return };
        let sc = self.scenes.remove(idx);
        if crate::nexus::is_disk_scene(&sc.id) {
            let _ = crate::nexus::delete_scene(&sc);
        }
        let was_in: Vec<String> = self.sets_of(&id).iter().map(|s| s.name.clone()).collect();
        self.sel.scene = self.scenes.first().map(|s| s.id.clone());
        self.draft = None;
        self.dirty = false;
        self.modal = AppModal::None;
        let msg = if was_in.is_empty() {
            self.trf("Deleted {0}", &[&sc.name])
        } else {
            self.trf("Deleted {0} — {1} shows a broken slot", &[&sc.name, &was_in.join(", ")])
        };
        self.toast_undo(msg, UndoAction::RestoreScene { scene: sc, idx });
    }

    pub fn addto_set_yes(&mut self, set_id: &str) {
        let Some(scene) = self.sel.scene.clone() else { return };
        if let Some(st) = self.sets.iter_mut().find(|s| s.id == set_id) {
            if !st.members.contains(&scene) {
                st.members.push(scene);
            }
            let nm = st.name.clone();
            self.sel.set = Some(set_id.into());
            self.modal = AppModal::None;
            let m = self.trf("Added to {0}", &[&nm]);
            self.toast(m);
        }
    }

    pub fn save(&mut self) {
        let Some(d) = self.draft.take() else { return };
        let nm = d.name.clone();
        if let Some(id) = self.sel.scene.clone() {
            if let Some(s) = self.scene_mut(&id) {
                let keep_id = s.id.clone();
                *s = d;
                s.id = keep_id;
            }
        }
        self.dirty = false;
        // Write through to the real dorobot-nexus scene library.
        let persisted = self
            .sel
            .scene
            .clone()
            .and_then(|id| self.scene(&id).cloned())
            .map(|sc| crate::nexus::save_scene(&sc));
        let m = match persisted {
            Some(Ok(p)) => self.trf("Saved {0}", &[&format!("{nm} → {}", p.display())]),
            Some(Err(e)) => self.trf("Saved {0}", &[&format!("{nm} (disk write failed: {e})")]),
            None => self.trf("Saved {0}", &[&nm]),
        };
        self.toast(m);
    }

    pub fn saveas(&mut self) {
        let sc = self.cur_scene();
        let terrain = if sc.terrain.is_empty() { "flat" } else { &sc.terrain };
        let name = format!(
            "{terrain}-f{}-kp{}{}",
            f2(sc.friction),
            f2(sc.kp),
            if sc.push > 0.0 { "-push" } else { "" }
        );
        let id = self.nid("s");
        let mut s = sc;
        s.id = id.clone();
        s.name = name.clone();
        let _ = crate::nexus::save_scene(&s);
        self.scenes.push(s);
        self.sel.scene = Some(id);
        self.draft = None;
        self.dirty = false;
        let m = self.trf("Saved as {0} — original untouched", &[&name]);
        self.toast(m);
    }

    pub fn revert(&mut self) {
        self.draft = None;
        self.dirty = false;
        let m = self.tr("Reverted — re-simulating the saved scene");
        self.toast(m);
    }

    /// Begin (or continue) editing: materialize the draft.
    pub fn mark_dirty(&mut self) {
        if self.draft.is_none() {
            self.draft = Some(self.cur_scene());
        }
        self.dirty = true;
    }

    pub fn edit_field(&mut self, f: &str, v: f64) {
        self.mark_dirty();
        if let Some(d) = &mut self.draft {
            match f {
                "stance" => d.stance = v,
                "vx" => d.vx = v,
                "dur" => d.dur = v,
                "friction" => d.friction = v,
                "kp" => d.kp = v,
                "mass" => d.mass = v,
                "push" => d.push = v,
                "amp" => d.amp = v,
                "slope" => d.slope = v,
                _ => {}
            }
        }
    }

    pub fn edit_terrain(&mut self, fam: &str) {
        self.mark_dirty();
        if let Some(d) = &mut self.draft {
            d.terrain = fam.into();
        }
        self.resim();
    }

    pub fn toggle_force_fam(&mut self) {
        self.mark_dirty();
        if let Some(d) = &mut self.draft {
            d.force_fam = !d.force_fam;
        }
    }
    pub fn toggle_spawn(&mut self) {
        self.mark_dirty();
        if let Some(d) = &mut self.draft {
            d.spawn_dr = !d.spawn_dr;
        }
    }

    pub fn resim(&mut self) {
        let sc = self.cur_scene();
        self.live.frame = 0;
        self.live.frames = (sc.dur * 50.0).round() as u32;
        let terrain = if sc.terrain.is_empty() { self.tr("flat") } else { self.tr(&sc.terrain) };
        let frames = self.live.frames.to_string();
        let m = self.trf("Re-simulated on {0} — {1} frames", &[&terrain, &frames]);
        self.toast(m);
    }

    pub fn replay(&mut self, id: &str) {
        if let Some(r) = self.recordings.iter().find(|r| r.id == id) {
            self.live.frames = r.frames;
            self.live.frame = 0;
            self.live.playing = true;
            let nm = r.name.clone();
            let m = self.trf("Replaying {0} — same parser as a live rollout", &[&nm]);
            self.toast(m);
        }
    }

    pub fn del_rec(&mut self, id: &str) {
        if let Some(idx) = self.recordings.iter().position(|r| r.id == id) {
            let rec = self.recordings.remove(idx);
            let m = self.trf("Deleted recording {0}", &[&rec.name]);
            self.toast_undo(m, UndoAction::RestoreRecording { rec, idx });
        }
    }

    pub fn record_probe(&mut self) {
        let sc = self.cur_scene();
        let n = self.recordings.len() + 1;
        let id = self.nid("rec");
        let rec = Recording {
            id,
            name: format!("{}-{:03}", sc.name, n),
            scene: sc.name.clone(),
            frames: self.live.frames,
            resets: 4,
            dist: 1.62,
            path: None,
        };
        let nm = rec.name.clone();
        self.recordings.push(rec);
        let m = self.trf("Recorded {0} — tagged with its scene", &[&nm]);
        self.toast(m);
    }

    // ============================================================== composer

    pub fn comp_add(&mut self) {
        let (Some(set_id), Some(scene_id)) = (self.sel.set.clone(), self.sel.scene.clone()) else { return };
        let scene_name = self.scene(&scene_id).map(|s| s.name.clone()).unwrap_or_default();
        if let Some(st) = self.sets.iter_mut().find(|s| s.id == set_id) {
            if st.members.contains(&scene_id) {
                let nm = st.name.clone();
                let m = self.trf("Already in {0}", &[&nm]);
                self.toast(m);
                return;
            }
            st.members.push(scene_id);
            let nm = st.name.clone();
            let m = self.trf("Added {0} to {1}", &[&scene_name, &nm]);
            self.toast(m);
        }
    }

    pub fn comp_rm(&mut self, i: usize) {
        let Some(set_id) = self.sel.set.clone() else { return };
        if let Some(st) = self.sets.iter_mut().find(|s| s.id == set_id) {
            if i >= st.members.len() {
                return;
            }
            let gone = st.members.remove(i);
            let nm = st.name.clone();
            let m = self.trf("Removed from {0} — the scene itself survives", &[&nm]);
            self.toast_undo(m, UndoAction::RestoreMember { set: set_id, idx: i, member: gone });
        }
    }

    pub fn comp_move(&mut self, i: usize, right: bool) {
        let Some(set_id) = self.sel.set.clone() else { return };
        if let Some(st) = self.sets.iter_mut().find(|s| s.id == set_id) {
            let j = if right { i + 1 } else { i.wrapping_sub(1) };
            if i < st.members.len() && j < st.members.len() {
                st.members.swap(i, j);
            }
        }
    }

    pub fn set_rename_yes(&mut self, name: &str) {
        let Some(set_id) = self.sel.set.clone() else { return };
        if let Some(st) = self.sets.iter_mut().find(|s| s.id == set_id) {
            st.name = if name.is_empty() { "unnamed".into() } else { name.into() };
        }
        self.modal = AppModal::None;
        let m = self.tr("Renamed");
        self.toast(m);
    }

    pub fn set_dup(&mut self) {
        let Some(set_id) = self.sel.set.clone() else { return };
        let Some(st) = self.set(&set_id).cloned() else { return };
        let id = self.nid("t");
        self.sets.push(SceneSet { id: id.clone(), name: format!("{} copy", st.name), members: st.members });
        self.sel.set = Some(id);
        let m = self.tr("Duplicated set — same scenes, new ordering to edit");
        self.toast(m);
    }

    // ================================================================= train

    pub fn launch(&mut self, name: &str, warm: &str) {
        self.modal = AppModal::None;
        for r in &mut self.runs {
            if r.state == RunState::Running {
                r.state = RunState::Stopped;
            }
        }
        let Some(set_id) = self.sel.set.clone() else { return };
        let Some(st) = self.set(&set_id).cloned() else { return };
        let name = if name.is_empty() { "run" } else { name };
        let cfg: Vec<CfgScene> = st
            .members
            .iter()
            .filter_map(|id| self.scene(id))
            .map(|sc| CfgScene {
                name: sc.name.clone(),
                stance: sc.stance,
                terrain: sc.terrain.clone(),
                friction: sc.friction,
                kp: sc.kp,
                mass: sc.mass,
                push: sc.push,
            })
            .collect();
        let stages: Vec<String> = st
            .members
            .iter()
            .map(|id| self.scene(id).map(|s| s.name.clone()).unwrap_or_else(|| "?".into()))
            .collect();
        let id = self.nid("run");
        self.runs.insert(0, Run {
            id: id.clone(),
            name: name.into(),
            set: st.name.clone(),
            state: RunState::Running,
            archived: false,
            coll: false,
            steps: 0.0,
            best: Some(-0.42),
            stage: 0,
            stages,
            round: 1,
            iter: 0,
            iters_per: 400,
            snapshot: Snapshot { scenes: st.members.len(), seed: "0xC0FFEE".into(), warm: Some(warm.into()), cfg },
            ckpts: vec![],
            diagnosis: None,
        });
        self.sel.run = Some(id);
        let msg = format!("{} · {}", self.trf("Launched {0} — snapshot embedded", &[name]), self.tr(warm));
        self.toast(msg);
        self.mode = Mode::Train;
    }

    pub fn pause(&mut self) {
        let Some(id) = self.sel.run.clone() else { return };
        if let Some(r) = self.run_mut(&id) {
            r.state = RunState::Paused;
        }
        let m = self.tr("Paused at the last checkpoint — up to 50 iterations (~3 min) will be re-done on resume");
        self.toast(m);
    }
    pub fn resume(&mut self) {
        let Some(id) = self.sel.run.clone() else { return };
        if let Some(r) = self.run_mut(&id) {
            r.state = RunState::Running;
        }
        let m = self.tr("Resumed from checkpoint");
        self.toast(m);
    }
    pub fn stop(&mut self) {
        let Some(id) = self.sel.run.clone() else { return };
        if let Some(r) = self.run_mut(&id) {
            r.state = RunState::Stopped;
        }
        let m = self.tr("Stopped — the record is now immutable");
        self.toast(m);
    }

    // =========================================================== checkpoints

    pub fn ck_promote_yes(&mut self, ck_id: &str, pname: &str) {
        let Some(run_id) = self.sel.run.clone() else { return };
        let pname = if pname.is_empty() { "promoted" } else { pname };
        if let Some(r) = self.run_mut(&run_id) {
            if let Some(c) = r.ckpts.iter_mut().find(|c| c.id == ck_id) {
                c.st = CkSt::Prom;
                c.pname = Some(pname.into());
            }
        }
        self.modal = AppModal::None;
        let m = self.trf("Promoted as {0}", &[pname]);
        self.toast(m);
    }

    pub fn ck_demote(&mut self, ck_id: &str) {
        let Some(run_id) = self.sel.run.clone() else { return };
        let run_name = self.run(&run_id).map(|r| r.name.clone()).unwrap_or_default();
        let mut label = None;
        if let Some(r) = self.run_mut(&run_id) {
            if let Some(c) = r.ckpts.iter_mut().find(|c| c.id == ck_id) {
                c.st = CkSt::Ord;
                c.pname = None;
                label = Some(c.label.clone());
            }
        }
        if let (Some(l), Some(dgck), Some(dgrun)) = (&label, &self.dg.ck, &self.dg.ck_run) {
            if dgck == l && *dgrun == run_name {
                self.dg.ck = None;
                self.dg.ck_run = None;
                self.dg.pname = None;
                let m = self.tr("Candidate policy was demoted — deploy eligibility revoked");
                self.dg_invalidate(Some(m));
            }
        }
        let m = self.tr("Demoted — now subject to the retention policy");
        self.toast(m);
    }

    pub fn ck_export(&mut self, ck_id: &str) {
        let Some(run_id) = self.sel.run.clone() else { return };
        if let Some(r) = self.run(&run_id) {
            if let Some(c) = r.ckpts.iter().find(|c| c.id == ck_id) {
                let l = c.label.clone();
                let m = self.trf("Exported {0}.onnx — export is an action, not a state", &[&l]);
                self.toast(m);
            }
        }
    }

    pub fn ck_del(&mut self, ck_id: &str) {
        let Some(run_id) = self.sel.run.clone() else { return };
        let run_name = self.run(&run_id).map(|r| r.name.clone()).unwrap_or_default();
        let Some(r) = self.run_mut(&run_id) else { return };
        let Some(i) = r.ckpts.iter().position(|c| c.id == ck_id) else { return };
        if r.ckpts[i].st == CkSt::Prom {
            let pn = r.ckpts[i].pname.clone().unwrap_or_default();
            let m = self.trf("Blocked: promoted as “{0}” — demote first", &[&pn]);
            self.toast(m);
            return;
        }
        let ck = r.ckpts.remove(i);
        if self.dg.ck.as_deref() == Some(&ck.label) && self.dg.ck_run.as_deref() == Some(&run_name) {
            self.dg.ck = None;
            self.dg.ck_run = None;
            self.dg.pname = None;
            let m = self.tr("Candidate policy was deleted — pick another");
            self.dg_invalidate(Some(m));
        }
        let m = self.trf("Deleted {0}", &[&ck.label]);
        self.toast_undo(m, UndoAction::RestoreCkpt { run: run_id, idx: i, ck });
    }

    pub fn ck_inspect(&mut self, label: &str) {
        self.sel.ck = Some(label.into());
        self.sel.ck_run = self.sel.run.clone().and_then(|id| self.run(&id).map(|r| r.name.clone()));
        self.mode = Mode::Inspect;
        let m = self.trf("Probing {0}", &[label]);
        self.toast(m);
    }

    pub fn pick_ck(&mut self, run_name: &str, label: &str) {
        self.sel.ck = Some(label.into());
        self.sel.ck_run = Some(run_name.into());
        let m = self.trf("Loaded {0} — restart pulls any newer one", &[label]);
        self.toast(m);
    }

    // ================================================================= sweep

    pub fn sweep_run(&mut self) {
        self.sweep_grid = Some(SWEEP_REF);
        self.sweep_at = 0;
        self.sweep_state = SweepState::Running;
        self.sel.cell = None;
    }

    pub fn sweep_stop(&mut self) {
        self.sweep_state = SweepState::Aborted;
        let at = self.sweep_at.to_string();
        let m = self.trf("Aborted at {0}/40 — a partial surface is evidence, so it is kept and labelled", &[&at]);
        self.toast(m);
    }

    pub fn sweep_del(&mut self) {
        self.sweep_grid = None;
        self.sweep_state = SweepState::Idle;
        self.sweep_at = 0;
        self.sweep_baseline = false;
        self.sel.cell = None;
        let m = self.tr("Deleted — the crouch-v2 comparison now reads “baseline missing”, never a silent wrong number");
        self.toast(m);
    }

    pub fn sel_recipe(&mut self, i: usize) {
        if self.sel.recipe == i {
            return;
        }
        self.sel.recipe = i;
        self.sweep_grid = None;
        self.sweep_state = SweepState::Idle;
        self.sweep_at = 0;
        self.sel.cell = None;
        let m = self.tr("Recipe changed — the previous surface no longer applies; re-run the sweep");
        self.toast(m);
    }

    pub fn cell_inspect(&mut self) {
        if self.sel.recipe != 0 {
            let m = self.tr("This recipe's axes don't map to probe physics yet — available for PD × friction");
            self.toast(m);
            return;
        }
        let Some((ri, ci)) = self.sel.cell else { return };
        let rc = &RECIPES[0];
        self.cell_phys = Some((rc.fr[ri], rc.ga[ci]));
        self.mode = Mode::Inspect;
        let m = self.tr("Probe pre-loaded with the failing physics");
        self.toast(m);
    }

    pub fn cell_scene(&mut self) {
        if self.sel.recipe != 0 {
            let m = self.tr("This recipe's axes don't map to scene fields yet — available for PD × friction");
            self.toast(m);
            return;
        }
        let Some((ri, ci)) = self.sel.cell else { return };
        let rc = &RECIPES[0];
        let id = self.nid("s");
        let mut s = Scene::base(&id, &format!("edge-f{}-g{}", f2(rc.fr[ri]), f2(rc.ga[ci])));
        s.friction = rc.fr[ri];
        s.kp = rc.ga[ci];
        s.terrain = "rough".into();
        s.stance = 0.60;
        let nm = s.name.clone();
        self.scenes.push(s);
        let m = self.trf("Saved as scene “{0}” — the failing physics is now training material", &[&nm]);
        self.toast(m);
    }

    // ================================================================== runs

    pub fn rerun_yes(&mut self, name: &str) {
        let Some(src_id) = self.sel.run.clone() else { return };
        let Some(src) = self.run(&src_id).cloned() else { return };
        self.modal = AppModal::None;
        for r in &mut self.runs {
            if r.state == RunState::Running {
                r.state = RunState::Stopped;
            }
        }
        let id = self.nid("run");
        let mut r = src;
        r.id = id.clone();
        r.name = name.into();
        r.state = RunState::Running;
        r.archived = false;
        r.steps = 0.0;
        r.iter = 0;
        r.round = 1;
        r.stage = 0;
        r.ckpts = vec![];
        r.diagnosis = None;
        r.best = Some(-0.42);
        self.runs.insert(0, r);
        self.sel.run = Some(id);
        self.mode = Mode::Train;
        let m = self.tr("Reproducing from snapshot");
        self.toast(m);
    }

    pub fn run_archive(&mut self) {
        let Some(id) = self.sel.run.clone() else { return };
        if let Some(r) = self.run_mut(&id) {
            r.archived = true;
        }
        let m = self.tr("Archived — hidden from Home, dimmed in the ledger, fully retained");
        self.toast(m);
    }

    pub fn run_delete_yes(&mut self, keep: bool) {
        let Some(id) = self.sel.run.clone() else { return };
        let Some(i) = self.runs.iter().position(|r| r.id == id) else { return };
        let r = self.runs.remove(i);
        let keep = keep && !r.coll;
        if self.dg.ck.is_some() && self.dg.ck_run.as_deref() == Some(&r.name) {
            self.dg.ck = None;
            self.dg.ck_run = None;
            self.dg.pname = None;
            let m = self.tr("Candidate policy's run was deleted — pick another");
            self.dg_invalidate(Some(m));
        }
        let prom: Vec<Ckpt> = r.ckpts.iter().filter(|c| c.st == CkSt::Prom).cloned().collect();
        let mut took: Vec<String> = vec![];
        if keep && !prom.is_empty() {
            took = prom.iter().map(|c| c.id.clone()).collect();
            if let Some(k) = self.runs.iter_mut().find(|x| x.id == "kept") {
                for c in prom.iter().rev() {
                    k.ckpts.insert(0, c.clone());
                }
            } else {
                self.runs.push(Run {
                    id: "kept".into(),
                    name: "(kept checkpoints)".into(),
                    set: "—".into(),
                    state: RunState::Completed,
                    archived: true,
                    coll: true,
                    steps: 0.0,
                    best: None,
                    stage: 0,
                    stages: vec![],
                    round: 0,
                    iter: 0,
                    iters_per: 0,
                    snapshot: Snapshot { scenes: 0, seed: "—".into(), warm: None, cfg: vec![] },
                    ckpts: prom.clone(),
                    diagnosis: None,
                });
            }
        }
        self.sel.run = self.runs.first().map(|x| x.id.clone());
        self.modal = AppModal::None;
        let m = self.trf("Deleted {0}", &[&r.name]);
        self.toast_undo(m, UndoAction::RestoreRun { run: r, idx: i, took_from_kept: took });
    }

    // ================================================================ robots

    pub fn wiz_commit(&mut self) {
        let id = self.nid("r");
        let name = self
            .wiz_file
            .as_ref()
            .and_then(|p| std::path::Path::new(p).file_stem().map(|s| s.to_string_lossy().to_string()))
            .unwrap_or_else(|| "Unitree H2-plus".into());
        self.robots.push(Robot {
            id,
            name,
            links: 31,
            joints: 30,
            movable: 23,
            mapped: 12,
            meshes: 28,
            used_by: vec![],
            urdf: self.wiz_file.clone(),
        });
        self.modal = AppModal::None;
        let m = self.tr("Committed — assets copied into the library, never referenced");
        self.toast(m);
    }

    // ================================================================ deploy

    pub fn dep_pick_ck(&mut self, run_name: &str, label: &str) {
        if self.dg.ck.as_deref() == Some(label) && self.dg.ck_run.as_deref() == Some(run_name) {
            return;
        }
        let pname = self
            .runs
            .iter()
            .find(|r| r.name == run_name)
            .and_then(|r| r.ckpts.iter().find(|c| c.label == label))
            .and_then(|c| c.pname.clone())
            .unwrap_or_else(|| label.to_string());
        let had = GATES.iter().any(|g| *self.dg.gate(*g) != Gate::NotRun) || self.dg.chk.iter().any(|c| *c);
        self.dg.ck = Some(label.into());
        self.dg.ck_run = Some(run_name.into());
        self.dg.pname = Some(pname);
        self.sel.dep = None;
        let msg = had.then(|| self.tr("Gates reset — they certified the previous pair"));
        self.dg_invalidate(msg);
    }

    pub fn sel_target(&mut self, id: &str) {
        if self.dg.target.as_deref() == Some(id) {
            return;
        }
        let had = GATES.iter().any(|g| *self.dg.gate(*g) != Gate::NotRun) || self.dg.chk.iter().any(|c| *c);
        self.dg.target = Some(id.into());
        self.sel.dep = None;
        let msg = had.then(|| self.tr("Gates reset — they certified the previous pair"));
        self.dg_invalidate(msg);
    }

    pub fn target_save(&mut self, edit: Option<String>, name: &str, ip: &str, iface: &str, cap: u32) {
        let name = if name.is_empty() { "target" } else { name };
        match edit {
            Some(id) => {
                let was_candidate = self.dg.target.as_deref() == Some(id.as_str());
                if let Some(tg) = self.targets.iter_mut().find(|t| t.id == id) {
                    tg.name = name.into();
                    if !ip.is_empty() {
                        tg.ip = ip.into();
                    }
                    if !iface.is_empty() {
                        tg.iface = iface.into();
                    }
                    tg.cap = cap;
                    tg.rev += 1;
                    tg.ping = Ping::Unprobed;
                }
                if was_candidate {
                    let m = self.tr("Gates reset — the target changed under them");
                    self.dg_invalidate(Some(m));
                }
                let m = self.trf("Saved {0} — ping state cleared, it may have moved", &[name]);
                self.toast(m);
            }
            None => {
                let id = self.nid("tg");
                self.targets.push(Target {
                    id: id.clone(),
                    name: name.into(),
                    model: "G1-29dof".into(),
                    iface: if iface.is_empty() { "eth0".into() } else { iface.into() },
                    ip: if ip.is_empty() { "192.168.123.162".into() } else { ip.into() },
                    sdk: "unitree_sdk2 2.0".into(),
                    cap,
                    rev: 0,
                    ping: Ping::Unprobed,
                });
                self.dg.target = Some(id);
                let m = self.tr("Gates reset — the target changed under them");
                self.dg_invalidate(Some(m));
                let m2 = self.trf("Added {0}", &[name]);
                self.toast(m2);
            }
        }
        self.modal = AppModal::None;
    }

    pub fn target_del(&mut self) {
        let Some(id) = self.dg.target.clone() else { return };
        let Some(tg) = self.target(&id).cloned() else { return };
        if let Some(held) = self.deploys.iter().find(|d| d.tg_id == tg.id && d.state == DepState::Live) {
            self.modal = AppModal::RemoveTargetBlocked { held: held.name.clone() };
            return;
        }
        let Some(i) = self.targets.iter().position(|t| t.id == id) else { return };
        let tg = self.targets.remove(i);
        self.dg.target = self.targets.first().map(|t| t.id.clone());
        self.dg_invalidate(Some(self.tr("Gates reset — the target changed under them")));
        let m = self.trf("Removed {0}", &[&tg.name]);
        self.toast_undo(m, UndoAction::RestoreTarget { target: tg, idx: i });
    }

    pub fn target_ping(&mut self) {
        let Some(id) = self.dg.target.clone() else { return };
        if let Some(tg) = self.targets.iter_mut().find(|t| t.id == id) {
            tg.ping = Ping::Probing;
        }
    }

    fn ping_resolve(&mut self) {
        let mut msg = None;
        for tg in &mut self.targets {
            if tg.ping == Ping::Probing {
                if tg.iface.starts_with("eth") || tg.iface.starts_with("en") {
                    tg.ping = Ping::Ok(12);
                } else {
                    tg.ping = Ping::Bad;
                    msg = Some(tg.iface.clone());
                }
            }
        }
        if let Some(iface) = msg {
            let m = self.trf("No control route over {0} — wireless is refused for control; use the wired link", &[&iface]);
            self.toast(m);
        }
    }

    pub fn gate_start(&mut self, id: GateId) {
        // Sequential enforcement.
        let allowed = match id {
            GateId::Export => self.dg.ck.is_some(),
            GateId::Compat => self.dg.export.ok(),
            GateId::Sim2sim => self.dg.compat.ok(),
            GateId::Dryrun => {
                if !self.dg.sim2sim.ok() {
                    false
                } else {
                    let ok = self
                        .dg
                        .target
                        .as_ref()
                        .and_then(|t| self.target(t))
                        .map(|t| matches!(t.ping, Ping::Ok(_)))
                        .unwrap_or(false);
                    if !ok {
                        let m = self.tr("target unreachable — ping it first");
                        self.toast(m);
                        return;
                    }
                    true
                }
            }
        };
        if !allowed {
            return;
        }
        self.sel.dep = None;
        *self.dg.gate_mut(id) = Gate::Running(0);
    }

    pub fn gate_cancel(&mut self, id: GateId) {
        if self.dg.gate(id).running() {
            *self.dg.gate_mut(id) = Gate::Fail { why: Line::new("cancelled by operator — re-run when ready") };
        }
    }

    /// Advance running gates + pings one fast tick. Gate results carry the
    /// pair key they certified; a pair change mid-flight discards the work.
    pub fn gate_tick(&mut self) {
        self.ping_tick_counter = self.ping_tick_counter.wrapping_add(1);
        if self.ping_tick_counter % 6 == 0 {
            self.ping_resolve();
        }
        let key = self.pair_key();
        for id in GATES {
            let Gate::Running(p) = self.dg.gate(id) else { continue };
            let p = p + 1;
            let done = match id {
                GateId::Export => p >= 8,
                GateId::Compat => p >= 10,
                GateId::Sim2sim => p >= 24,
                GateId::Dryrun => p >= 15,
            };
            if !done {
                *self.dg.gate_mut(id) = Gate::Running(p);
                continue;
            }
            match id {
                GateId::Export => {
                    // Prefer the fingerprint of the real newest checkpoint on
                    // disk; fall back to the pair-derived demo hash.
                    let (h, kb) = match &self.real_artifact {
                        Some((h, kb)) => (h.clone(), *kb as u32),
                        None => {
                            let h = Store::fake_hash(&format!(
                                "{}|{}|{}",
                                self.dg.ck_run.clone().unwrap_or_default(),
                                self.dg.ck.clone().unwrap_or_default(),
                                self.dg.target.clone().unwrap_or_default()
                            ));
                            let kb = 396 + (u32::from_str_radix(&h, 16).unwrap_or(0) % 64);
                            (h, kb)
                        }
                    };
                    self.dg.export = Gate::Pass { key: key.clone(), hash: h, kb };
                    let m = self.tr("Exported — hash-stable across a re-export");
                    self.toast(m);
                }
                GateId::Compat => {
                    let cap = self.dg.target.as_ref().and_then(|t| self.target(t)).map(|t| t.cap).unwrap_or(70);
                    if cap < 60 {
                        self.dg.compat = Gate::Fail {
                            why: Line::arg(
                                "action range exceeds the {0}% profile envelope — raise the profile or choose a stronger target",
                                vec![cap.to_string()],
                            ),
                        };
                        let m = self.tr("Compatibility failed — see the gate for the reason");
                        self.toast(m);
                    } else {
                        self.dg.compat = Gate::Pass { key: key.clone(), hash: String::new(), kb: 0 };
                        let m = self.tr("Compatible — 5 checks, 1 accepted note");
                        self.toast(m);
                    }
                }
                GateId::Sim2sim => {
                    self.dg.sim2sim = Gate::Pass { key: key.clone(), hash: String::new(), kb: 0 };
                    let m = self.tr("Sim-to-sim passed — survival 96%, drift 6.8%");
                    self.toast(m);
                }
                GateId::Dryrun => {
                    self.dg.dryrun = Gate::Pass { key: key.clone(), hash: String::new(), kb: 0 };
                    let m = self.tr("Dry-run clean — the robot never moved");
                    self.toast(m);
                }
            }
        }
    }

    pub fn dep_chk(&mut self, i: usize, v: bool) {
        if i < 4 {
            self.dg.chk[i] = v;
        }
    }

    pub fn dep_reset(&mut self) {
        self.dg_invalidate(None);
        let m = self.tr("Candidate reset — gates and checklist cleared");
        self.toast(m);
    }

    pub fn arm_missing(&self) -> Option<&'static str> {
        if self.dg.ck.is_none() {
            Some("pick a promoted policy")
        } else if !self.dg.gates_ok() {
            Some("run the four gates")
        } else if !self.dg.chk_ok() {
            Some("complete the safety checklist")
        } else {
            None
        }
    }

    pub fn dep_arm_yes(&mut self) {
        self.modal = AppModal::None;
        let pk = self.pair_key();
        // Full revalidation: promotion, all four gates on THIS pair, checklist.
        let promoted = self
            .dg
            .ck_run
            .as_ref()
            .zip(self.dg.ck.as_ref())
            .and_then(|(rn, l)| {
                self.runs
                    .iter()
                    .find(|r| &r.name == rn)
                    .and_then(|r| r.ckpts.iter().find(|c| &c.label == l && c.st == CkSt::Prom))
            })
            .is_some();
        let gates_keyed = GATES.iter().all(|g| match self.dg.gate(*g) {
            Gate::Pass { key, .. } => *key == pk,
            _ => false,
        });
        let Some(tg) = self.dg.target.as_ref().and_then(|t| self.target(t)).cloned() else { return };
        if !promoted || !gates_keyed || !self.dg.chk_ok() {
            let m = self.tr("Arm blocked — certification is no longer valid");
            self.toast(m);
            return;
        }
        for d in &mut self.deploys {
            if d.state == DepState::Live && d.tg_id == tg.id {
                d.state = DepState::Superseded;
            }
        }
        let Gate::Pass { hash, kb, .. } = self.dg.export.clone() else { return };
        self.dep_serial += 1;
        let id = self.nid("d");
        let name = format!("dep-{:03}", self.dep_serial);
        self.deploys.insert(0, Deploy {
            id: id.clone(),
            name: name.clone(),
            ck: format!("{}|{}", self.dg.ck_run.clone().unwrap_or_default(), self.dg.ck.clone().unwrap_or_default()),
            pname: self.dg.pname.clone().unwrap_or_default(),
            target: tg.name.clone(),
            tg_id: tg.id.clone(),
            tg_rev: tg.rev,
            hash: hash.clone(),
            state: DepState::Live,
            stage: 0,
            tg_snap: Some(TgSnap { model: tg.model.clone(), ip: tg.ip.clone(), iface: tg.iface.clone(), sdk: tg.sdk.clone(), cap: tg.cap }),
            gate_rep: vec![
                Line::arg("onnx opset 17 · {0} KB · id {1}", vec![kb.to_string(), hash]),
                Line::new("compat 5/5 · 1 accepted note"),
                Line::new("sim2sim 24 eps · surv 96% · drift 6.8%"),
                Line::new("dry-run · sat 9% · jerk p99 0.4"),
            ],
            chk: self.dg.chk,
            signer: "operator".into(),
            ts: format!("t+{:.0}s", self.clock),
            pair: pk,
            note: None,
            events: vec![Line::arg("armed at {0}% torque cap", vec!["40".into()])],
        });
        self.dg.armed = Some(id);
        self.sel.dep = None;
        self.dg_live = DgLive { torq: 34, ..DgLive::default() };
        let m = self.trf("Armed {0} at 40% torque — advance when it settles", &[&name]);
        self.toast(m);
    }

    pub fn stage_adv(&mut self) {
        let Some(id) = self.dg.armed.clone() else { return };
        if self.dg_live.dwell < 4 {
            let m = self.tr("Not settled — hold at this stage a moment longer");
            self.toast(m);
            return;
        }
        let mut pct = None;
        if let Some(d) = self.deploys.iter_mut().find(|d| d.id == id) {
            if d.stage >= 2 {
                return;
            }
            d.stage += 1;
            let p = [40, 70, 100][d.stage];
            d.events.push(Line::arg("advanced to {0}% torque cap", vec![p.to_string()]));
            pct = Some(p);
        }
        self.dg_live.dwell = 0;
        self.dg_live.unlocked = false;
        if let Some(p) = pct {
            let ps = p.to_string();
            let m = self.trf("Advanced to {0}% torque cap", &[&ps]);
            self.toast(m);
        }
    }

    pub fn dep_done(&mut self) {
        let Some(id) = self.dg.armed.take() else { return };
        let mut nm = String::new();
        if let Some(d) = self.deploys.iter_mut().find(|d| d.id == id) {
            d.events.push(Line::new("rollout complete at 100%"));
            nm = d.name.clone();
        }
        let m = self.trf("{0} is live at 100% — the record is immutable", &[&nm]);
        self.toast(m);
    }

    fn estop_inner(&mut self, id: &str) {
        let mut nm = String::new();
        if let Some(d) = self.deploys.iter_mut().find(|d| d.id == id) {
            let p = [40, 70, 100][d.stage].to_string();
            d.state = DepState::Aborted;
            d.note = Some(Line::arg("software stop at {0}% — robot dropped to damping", vec![p.clone()]));
            d.events.push(Line::arg("software stop at {0}%", vec![p]));
            nm = d.name.clone();
        }
        if self.dg.armed.as_deref() == Some(id) {
            self.dg.armed = None;
        }
        let m = self.trf("E-STOP — damping. {0} recorded as aborted; gates stay valid for re-arm", &[&nm]);
        self.toast(m);
    }

    pub fn dep_estop(&mut self) {
        if let Some(id) = self.dg.armed.clone() {
            self.estop_inner(&id);
        }
    }
    pub fn dep_estop_rec(&mut self) {
        if let Some(id) = self.sel.dep.clone() {
            self.estop_inner(&id);
        }
    }

    pub fn dep_rollback(&mut self) {
        let Some(cur_id) = self.dg.armed.clone() else { return };
        let Some(cur) = self.deploy(&cur_id).cloned() else { return };
        let Some(prev) = self
            .deploys
            .iter()
            .find(|d| d.state == DepState::Superseded && d.tg_id == cur.tg_id)
            .cloned()
        else {
            return;
        };
        if let Some(c) = self.deploys.iter_mut().find(|d| d.id == cur_id) {
            c.state = DepState::RolledBack;
            c.note = Some(Line::arg("rolled back to {0}", vec![prev.name.clone()]));
            c.events.push(Line::arg("rolled back to {0}", vec![prev.name.clone()]));
        }
        self.dep_serial += 1;
        let id = self.nid("d");
        self.deploys.insert(0, Deploy {
            id: id.clone(),
            name: format!("dep-{:03}", self.dep_serial),
            ck: prev.ck.clone(),
            pname: prev.pname.clone(),
            target: prev.target.clone(),
            tg_id: prev.tg_id.clone(),
            tg_rev: prev.tg_rev,
            hash: prev.hash.clone(),
            state: DepState::Live,
            stage: 2,
            tg_snap: prev.tg_snap.clone(),
            gate_rep: prev.gate_rep.clone(),
            chk: prev.chk,
            signer: prev.signer.clone(),
            ts: prev.ts.clone(),
            pair: prev.pair.clone(),
            note: Some(Line::arg("re-armed by rollback of {0}", vec![cur.name.clone()])),
            events: vec![Line::arg("re-armed by rollback of {0}", vec![cur.name.clone()])],
        });
        self.dg.armed = Some(id);
        let m = self.trf("Rolled back — {0} re-armed at 100% from its recorded artifact", &[&prev.pname]);
        self.toast(m);
    }

    pub fn dep_retire(&mut self) {
        let Some(id) = self.sel.dep.clone() else { return };
        let mut nm = String::new();
        if let Some(d) = self.deploys.iter_mut().find(|d| d.id == id) {
            d.state = DepState::Retired;
            d.events.push(Line::new("retired"));
            nm = d.name.clone();
        }
        if self.dg.armed.as_deref() == Some(&id) {
            self.dg.armed = None;
        }
        let m = self.trf("Retired {0} — the robot returns to damping", &[&nm]);
        self.toast(m);
    }

    pub fn dep_recert(&mut self) {
        let Some(id) = self.sel.dep.clone() else { return };
        let Some(d) = self.deploy(&id).cloned() else { return };
        let parts: Vec<&str> = d.ck.splitn(2, '|').collect();
        let (rn, label) = (parts.first().copied().unwrap_or(""), parts.get(1).copied().unwrap_or(""));
        let still = self
            .runs
            .iter()
            .find(|r| r.name == rn)
            .and_then(|r| r.ckpts.iter().find(|c| c.label == label && c.st == CkSt::Prom))
            .is_some();
        if !still {
            let m = self.tr("Blocked: that checkpoint is gone or no longer promoted — promote it again in Train");
            self.toast(m);
            return;
        }
        self.dg.ck = Some(label.into());
        self.dg.ck_run = Some(rn.into());
        self.dg.pname = Some(d.pname.clone());
        self.dg_invalidate(None);
        self.sel.dep = None;
        let m = self.trf("Loaded {0} as candidate — gates must re-certify", &[&d.pname]);
        self.toast(m);
    }

    pub fn ck_deploy(&mut self, ck_id: &str) {
        let Some(run_id) = self.sel.run.clone() else { return };
        let Some((run_name, label, pname)) = self.run(&run_id).and_then(|r| {
            r.ckpts.iter().find(|c| c.id == ck_id).map(|c| {
                (r.name.clone(), c.label.clone(), c.pname.clone().unwrap_or_else(|| c.label.clone()))
            })
        }) else { return };
        self.dg.ck = Some(label);
        self.dg.ck_run = Some(run_name);
        self.dg.pname = Some(pname);
        self.sel.dep = None;
        self.dg_invalidate(None);
        self.mode = Mode::Deploy;
        let m = self.tr("Candidate loaded — four gates and a checklist before hardware");
        self.toast(m);
    }

    // ================================================================== undo

    pub fn apply_undo(&mut self, u: UndoAction) {
        match u {
            UndoAction::RestoreScene { scene, idx } => {
                let nm = scene.name.clone();
                let id = scene.id.clone();
                let idx = idx.min(self.scenes.len());
                self.scenes.insert(idx, scene);
                self.sel.scene = Some(id);
                let m = self.trf("Restored {0}", &[&nm]);
                self.toast(m);
            }
            UndoAction::RestoreRecording { rec, idx } => {
                let idx = idx.min(self.recordings.len());
                self.recordings.insert(idx, rec);
            }
            UndoAction::RestoreRun { run, idx, took_from_kept } => {
                if !took_from_kept.is_empty() {
                    if let Some(k) = self.runs.iter_mut().find(|x| x.id == "kept") {
                        k.ckpts.retain(|c| !took_from_kept.contains(&c.id));
                    }
                    self.runs.retain(|x| x.id != "kept" || !x.ckpts.is_empty());
                }
                let id = run.id.clone();
                let idx = idx.min(self.runs.len());
                self.runs.insert(idx, run);
                self.sel.run = Some(id);
            }
            UndoAction::RestoreTarget { target, idx } => {
                let id = target.id.clone();
                let idx = idx.min(self.targets.len());
                self.targets.insert(idx, target);
                self.dg.target = Some(id);
                self.dg_invalidate(None);
            }
            UndoAction::RestoreMember { set, idx, member } => {
                if let Some(st) = self.sets.iter_mut().find(|s| s.id == set) {
                    let idx = idx.min(st.members.len());
                    st.members.insert(idx, member);
                }
            }
            UndoAction::RestoreCkpt { run, idx, ck } => {
                if let Some(r) = self.run_mut(&run) {
                    let idx = idx.min(r.ckpts.len());
                    r.ckpts.insert(idx, ck);
                }
            }
        }
    }

    // ================================================================= ticks

    /// ~120ms: playback frames, gate/ping progress, sweep reveal, toast expiry.
    pub fn fast_tick(&mut self) {
        self.clock += 0.12;
        // A loaded rollout advances on its own 25 Hz timer, one frame at a
        // time; stepping it here as well would play it at double speed.
        if self.live.playing && !self.replay_driven {
            self.live.frame = (self.live.frame + 3) % self.live.frames.max(1);
        }
        self.gate_tick();
        if self.sweep_state == SweepState::Running {
            self.sweep_at += 1;
            if self.sweep_at >= 40 {
                self.sweep_at = 40;
                self.sweep_state = SweepState::Complete;
                let p = self.sweep_pass().to_string();
                let m = self.trf("Sweep complete — {0}% of cells pass", &[&p]);
                self.toast(m);
            }
        }
        let now = self.clock;
        self.toasts.retain(|t| now - t.born < if t.undo.is_some() { 6.0 } else { 3.2 });
    }

    /// ~1.1s: training progress, checkpoints + retention, rollout telemetry.
    pub fn train_tick(&mut self) {
        // Rollout telemetry + health-gated dwell.
        if let Some(armed) = self.dg.armed.clone() {
            if let Some(d) = self.deploy(&armed) {
                let stage = d.stage;
                let cap = [40, 70, 100][stage];
                self.dg_live.tick += 1;
                let t = self.dg_live.tick as f64;
                self.dg_live.torq = [34, 48, 64][stage] + ((t * 1.3).sin() * 4.0).round() as i32;
                self.dg_live.herr = (t * 0.7).sin().abs() * 0.011;
                if self.dg_live.torq <= cap {
                    self.dg_live.dwell += 1;
                } else {
                    self.dg_live.dwell = 0;
                    self.dg_live.unlocked = false;
                }
                if self.dg_live.dwell >= 4 {
                    self.dg_live.unlocked = true;
                }
            }
        }
        let Some(run_id) = self.runs.iter().find(|r| r.state == RunState::Running).map(|r| r.id.clone()) else {
            return;
        };
        let mut wrote_ck = None;
        let mut completed = None;
        {
            let target_stance = {
                let r = self.run(&run_id).unwrap();
                r.snapshot
                    .cfg
                    .get(r.stage)
                    .map(|c| c.stance)
                    .unwrap_or_else(|| self.cur_scene().stance)
            };
            let reward = self.live.reward;
            let r = self.run_mut(&run_id).unwrap();
            r.iter += 6;
            r.steps += 0.012;
            if r.iter >= r.iters_per {
                r.iter = 0;
                r.stage += 1;
                if r.stage >= r.stages.len() {
                    r.stage = 0;
                    r.round += 1;
                }
                if r.round > 6 {
                    r.state = RunState::Completed;
                    completed = Some(r.name.clone());
                }
            }
            if r.iter % 120 == 0 && r.iter > 0 {
                let label = format!("ck-{:.0}k", r.steps * 1000.0);
                for c in &mut r.ckpts {
                    if c.st == CkSt::Best {
                        c.st = CkSt::Ord;
                    }
                }
                r.ckpts.insert(0, Ckpt {
                    id: format!("c{}", r.iter as u64 + (r.steps * 1e6) as u64),
                    label: label.clone(),
                    score: reward,
                    st: CkSt::Best,
                    pname: None,
                });
                // Retention, enforced: last 4 ordinary + best + all promoted.
                let mut ord = 0;
                r.ckpts.retain(|c| match c.st {
                    CkSt::Prom | CkSt::Best => true,
                    CkSt::Ord => {
                        ord += 1;
                        ord <= 4
                    }
                });
                wrote_ck = Some(label);
            }
            let _ = target_stance;
        }
        let target_stance = {
            let r = self.run(&run_id).unwrap();
            r.snapshot
                .cfg
                .get(r.stage)
                .map(|c| c.stance)
                .unwrap_or_else(|| self.cur_scene().stance)
        };
        let it = self.run(&run_id).map(|r| r.iter).unwrap_or(0) as f64;
        self.live.reward = (self.live.reward + 0.0011 + it.sin() * 0.0004).min(-0.09);
        self.live.falls = (self.live.falls - 0.05).max(4.0);
        self.live.hist.push(self.live.reward);
        if self.live.hist.len() > 90 {
            self.live.hist.remove(0);
        }
        self.live.now += (target_stance - self.live.now) * 0.02;
        self.live.now_hist.push(self.live.now);
        if self.live.now_hist.len() > 90 {
            self.live.now_hist.remove(0);
        }
        if let Some(label) = wrote_ck {
            if self.mode == Mode::Train {
                let m = self.trf("Checkpoint {0} written", &[&label]);
                self.toast(m);
            }
        }
        if let Some(name) = completed {
            let m = self.trf("{0} completed", &[&name]);
            self.toast(m);
        }
    }
}

impl Store {
    /// Merge the real dorobot-nexus scene library and recordings into the
    /// seeded state. Disk entries keep `disk:` ids so deletes write through.
    pub fn merge_disk(&mut self) {
        crate::nexus::init_env();
        for s in crate::nexus::scene::list() {
            let ui = crate::nexus::to_ui(&s);
            if !self.scenes.iter().any(|x| x.id == ui.id) {
                self.scenes.push(ui);
            }
        }
        for r in crate::nexus::scene::Recording::list() {
            let id = format!("disk:{}", r.name);
            if !self.recordings.iter().any(|x| x.id == id) {
                self.recordings.push(Recording {
                    id,
                    name: r.name.clone(),
                    scene: r.scene.clone(),
                    frames: r.frames as u32,
                    resets: r.resets as u32,
                    dist: r.distance as f64,
                    path: Some(format!("{}/{}", crate::nexus::repo_dir(), r.rollout.display())),
                });
            }
        }
        self.real_artifact = crate::nexus::real_ckpts()
            .first()
            .and_then(|c| crate::nexus::hash_file(&c.path));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_gates(s: &mut Store) {
        s.gate_start(GateId::Export);
        for _ in 0..10 {
            s.fast_tick();
        }
        s.gate_start(GateId::Compat);
        for _ in 0..12 {
            s.fast_tick();
        }
        s.gate_start(GateId::Sim2sim);
        for _ in 0..26 {
            s.fast_tick();
        }
        s.target_ping();
        for _ in 0..8 {
            s.fast_tick();
        }
        s.gate_start(GateId::Dryrun);
        for _ in 0..17 {
            s.fast_tick();
        }
    }

    #[test]
    fn full_deploy_lifecycle() {
        let mut s = Store::seed();
        s.dep_pick_ck("crouch-v2", "ck-3200k");
        assert_eq!(s.dg.pname.as_deref(), Some("crouch-v2-final"));
        run_gates(&mut s);
        assert!(s.dg.gates_ok(), "gates: {:?} {:?} {:?} {:?}", s.dg.export, s.dg.compat, s.dg.sim2sim, s.dg.dryrun);
        for i in 0..4 {
            s.dep_chk(i, true);
        }
        assert!(s.arm_missing().is_none());
        s.dep_arm_yes();
        let dep = s.deploys[0].clone();
        assert_eq!(s.dg.armed.as_deref(), Some(dep.id.as_str()));
        assert!(dep.tg_snap.is_some());
        assert_eq!(dep.gate_rep.len(), 4);
        // dwell gating
        s.stage_adv();
        assert_eq!(s.deploys[0].stage, 0);
        s.dg_live.dwell = 4;
        s.stage_adv();
        assert_eq!(s.deploys[0].stage, 1);
        s.dg_live.dwell = 4;
        s.stage_adv();
        assert_eq!(s.deploys[0].stage, 2);
        // rollback to the seeded superseded record
        s.dep_rollback();
        assert_eq!(s.deploys[0].state, DepState::Live);
        assert_eq!(s.deploys[0].stage, 2);
        assert_eq!(s.deploys[0].pname, "crouch-v2-final");
        // e-stop from record view
        let live_id = s.deploys[0].id.clone();
        s.dep_done();
        s.sel.dep = Some(live_id);
        s.dep_estop_rec();
        assert_eq!(s.deploys[0].state, DepState::Aborted);
    }

    #[test]
    fn compat_fails_on_weak_profile() {
        let mut s = Store::seed();
        s.dep_pick_ck("crouch-v2", "ck-3200k");
        s.sel_target("tg2"); // cap 50
        s.gate_start(GateId::Export);
        for _ in 0..10 {
            s.fast_tick();
        }
        s.gate_start(GateId::Compat);
        for _ in 0..12 {
            s.fast_tick();
        }
        assert!(matches!(s.dg.compat, Gate::Fail { .. }));
    }

    #[test]
    fn pair_change_discards_inflight_gate() {
        let mut s = Store::seed();
        s.dep_pick_ck("crouch-v2", "ck-3200k");
        s.gate_start(GateId::Export);
        for _ in 0..3 {
            s.fast_tick();
        }
        s.sel_target("tg2"); // resets + gen bump
        for _ in 0..12 {
            s.fast_tick();
        }
        assert_eq!(s.dg.export, Gate::NotRun);
    }

    #[test]
    fn cancel_sticks() {
        let mut s = Store::seed();
        s.dep_pick_ck("crouch-v2", "ck-3200k");
        s.gate_start(GateId::Export);
        for _ in 0..3 {
            s.fast_tick();
        }
        s.gate_cancel(GateId::Export);
        for _ in 0..12 {
            s.fast_tick();
        }
        assert!(matches!(s.dg.export, Gate::Fail { .. }));
    }

    #[test]
    fn arm_blocked_on_stale_promotion() {
        let mut s = Store::seed();
        s.dep_pick_ck("crouch-v2", "ck-3200k");
        run_gates(&mut s);
        for i in 0..4 {
            s.dep_chk(i, true);
        }
        // demote directly (simulating another surface)
        s.runs[1].ckpts[0].st = CkSt::Ord;
        let n = s.deploys.len();
        s.dep_arm_yes();
        assert_eq!(s.deploys.len(), n);
        assert!(s.dg.armed.is_none());
    }

    #[test]
    fn target_edit_resets_gates() {
        let mut s = Store::seed();
        s.dep_pick_ck("crouch-v2", "ck-3200k");
        run_gates(&mut s);
        assert!(s.dg.gates_ok());
        s.target_save(Some("tg1".into()), "g1-lab-A2", "", "", 70);
        assert!(!s.dg.gates_ok());
        assert!(!s.dg.chk.iter().any(|c| *c));
    }

    #[test]
    fn run_delete_keeps_promoted_and_undo_dedups() {
        let mut s = Store::seed();
        s.sel.run = Some("run2".into());
        s.run_delete_yes(true);
        let kept = s.runs.iter().find(|r| r.id == "kept").unwrap();
        assert!(kept.coll && kept.ckpts.len() == 1);
        let undo = s.toasts.last().unwrap().undo.clone().unwrap();
        s.apply_undo(undo);
        assert!(s.runs.iter().all(|r| r.id != "kept"));
        assert!(s.runs.iter().any(|r| r.id == "run2"));
        // collection delete is final
        s.sel.run = Some("run2".into());
        s.run_delete_yes(true);
        s.sel.run = Some("kept".into());
        s.run_delete_yes(true);
        assert!(s.runs.iter().all(|r| r.id != "kept"));
    }

    #[test]
    fn last_scene_protected_and_training_survives() {
        let mut s = Store::seed();
        while s.scenes.len() > 1 {
            let id = s.scenes.last().unwrap().id.clone();
            s.sel.scene = Some(id);
            s.del_scene();
            if s.modal == AppModal::DeleteScene {
                s.del_scene_yes();
            }
        }
        assert_eq!(s.scenes.len(), 1);
        s.del_scene();
        assert_eq!(s.modal, AppModal::LastSceneBlocked);
        for _ in 0..5 {
            s.train_tick();
        }
    }

    #[test]
    fn retention_enforced() {
        let mut s = Store::seed();
        // run3 running; force many checkpoint writes
        for _ in 0..600 {
            s.train_tick();
        }
        let r = s.runs.iter().find(|r| r.name == "crouch-v3").unwrap();
        let ord = r.ckpts.iter().filter(|c| c.st == CkSt::Ord).count();
        assert!(ord <= 4, "ord = {ord}");
    }
}
