//! The entire application state and its transitions, ported 1:1 from the
//! frozen web mockup. Pure Rust — no makepad types — so every rule the
//! external reviews hardened (pair-keyed certification, generation tokens,
//! dwell gating, retention, blast radius) is unit-testable here.

#![allow(clippy::too_many_arguments)]

// ---------------------------------------------------------------- scenes --

#[derive(Clone, Debug)]
pub struct Scene {
    pub id: String,
    pub name: String,
    pub terrain: String, // "", "boxes", "rough", "wave", "step"
    pub stance: f64,
    pub vx: f64,
    pub dur: f64,
    pub friction: f64,
    pub kp: f64,
    pub mass: f64,
    pub push: f64,
    pub amp: f64,
    pub slope: f64,
    pub seed: String,
    pub force_fam: bool,
    pub spawn_dr: bool,
}

impl Scene {
    pub fn base(id: &str, name: &str) -> Self {
        Scene {
            id: id.into(),
            name: name.into(),
            terrain: String::new(),
            stance: 0.82,
            vx: 0.0,
            dur: 6.0,
            friction: 1.0,
            kp: 1.0,
            mass: 1.0,
            push: 0.0,
            amp: 1.0,
            slope: 0.0,
            seed: "0xC0FFEE".into(),
            force_fam: false,
            spawn_dr: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SceneSet {
    pub id: String,
    pub name: String,
    pub members: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Robot {
    pub id: String,
    pub name: String,
    pub links: u32,
    pub joints: u32,
    pub movable: u32,
    pub mapped: u32,
    pub meshes: u32,
    pub used_by: Vec<String>,
    pub urdf: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Recording {
    pub id: String,
    pub name: String,
    pub scene: String,
    pub frames: u32,
    pub resets: u32,
    pub dist: f64,
    /// Real rollout JSON path when this recording exists on disk.
    pub path: Option<String>,
}

// ------------------------------------------------------------------ runs --

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunState {
    Running,
    Paused,
    Stopped,
    Completed,
    Failed,
}

impl RunState {
    pub fn key(self) -> &'static str {
        match self {
            RunState::Running => "running",
            RunState::Paused => "paused",
            RunState::Stopped => "stopped",
            RunState::Completed => "completed",
            RunState::Failed => "failed",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CkSt {
    Ord,
    Best,
    Prom,
}

#[derive(Clone, Debug)]
pub struct Ckpt {
    pub id: String,
    pub label: String,
    pub score: f64,
    pub st: CkSt,
    pub pname: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CfgScene {
    pub name: String,
    pub stance: f64,
    pub terrain: String,
    pub friction: f64,
    pub kp: f64,
    pub mass: f64,
    pub push: f64,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub scenes: usize,
    pub seed: String,
    pub warm: Option<String>,
    pub cfg: Vec<CfgScene>,
}

#[derive(Clone, Debug)]
pub struct Run {
    pub id: String,
    pub name: String,
    pub set: String,
    pub state: RunState,
    pub archived: bool,
    /// The "(kept checkpoints)" artifact collection, not a re-runnable run.
    pub coll: bool,
    pub steps: f64,
    pub best: Option<f64>,
    pub stage: usize,
    pub stages: Vec<String>,
    pub round: u32,
    pub iter: u32,
    pub iters_per: u32,
    pub snapshot: Snapshot,
    pub ckpts: Vec<Ckpt>,
    pub diagnosis: Option<String>,
}

// ---------------------------------------------------------------- deploy --

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ping {
    Unprobed,
    Probing,
    Ok(u32),
    Bad,
}

#[derive(Clone, Debug)]
pub struct Target {
    pub id: String,
    pub name: String,
    pub model: String,
    pub iface: String,
    pub ip: String,
    pub sdk: String,
    pub cap: u32,
    pub rev: u32,
    pub ping: Ping,
}

/// A localizable line: translation key + already-formatted args.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub k: &'static str,
    pub a: Vec<String>,
}

impl Line {
    pub fn new(k: &'static str) -> Self {
        Line { k, a: vec![] }
    }
    pub fn arg(k: &'static str, a: Vec<String>) -> Self {
        Line { k, a }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DepState {
    Live,
    Aborted,
    RolledBack,
    Superseded,
    Retired,
}

impl DepState {
    pub fn key(self) -> &'static str {
        match self {
            DepState::Live => "live",
            DepState::Aborted => "aborted",
            DepState::RolledBack => "rolled-back",
            DepState::Superseded => "superseded",
            DepState::Retired => "retired",
        }
    }
}

#[derive(Clone, Debug)]
pub struct TgSnap {
    pub model: String,
    pub ip: String,
    pub iface: String,
    pub sdk: String,
    pub cap: u32,
}

#[derive(Clone, Debug)]
pub struct Deploy {
    pub id: String,
    pub name: String,
    pub ck: String, // "run|label"
    pub pname: String,
    pub target: String,
    pub tg_id: String,
    pub tg_rev: u32,
    pub hash: String,
    pub state: DepState,
    pub stage: usize, // 0,1,2 → 40/70/100%
    pub tg_snap: Option<TgSnap>,
    pub gate_rep: Vec<Line>,
    pub chk: [bool; 4],
    pub signer: String,
    pub ts: String,
    pub pair: String,
    pub note: Option<Line>,
    pub events: Vec<Line>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Gate {
    NotRun,
    Running(u32), // progress unit: compat 0..5, sim2sim 0..24, export/dryrun ticks
    Pass { key: String, hash: String, kb: u32 },
    Fail { why: Line },
}

impl Gate {
    pub fn ok(&self) -> bool {
        matches!(self, Gate::Pass { .. })
    }
    pub fn running(&self) -> bool {
        matches!(self, Gate::Running(_))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GateId {
    Export,
    Compat,
    Sim2sim,
    Dryrun,
}
pub const GATES: [GateId; 4] = [GateId::Export, GateId::Compat, GateId::Sim2sim, GateId::Dryrun];

#[derive(Clone, Debug, Default)]
pub struct Dg {
    pub ck: Option<String>,
    pub ck_run: Option<String>,
    pub pname: Option<String>,
    pub target: Option<String>,
    pub gen: u64,
    pub export: Gate,
    pub compat: Gate,
    pub sim2sim: Gate,
    pub dryrun: Gate,
    pub chk: [bool; 4],
    pub armed: Option<String>,
}

impl Default for Gate {
    fn default() -> Self {
        Gate::NotRun
    }
}

impl Dg {
    pub fn gate(&self, id: GateId) -> &Gate {
        match id {
            GateId::Export => &self.export,
            GateId::Compat => &self.compat,
            GateId::Sim2sim => &self.sim2sim,
            GateId::Dryrun => &self.dryrun,
        }
    }
    pub fn gate_mut(&mut self, id: GateId) -> &mut Gate {
        match id {
            GateId::Export => &mut self.export,
            GateId::Compat => &mut self.compat,
            GateId::Sim2sim => &mut self.sim2sim,
            GateId::Dryrun => &mut self.dryrun,
        }
    }
    pub fn gates_ok(&self) -> bool {
        GATES.iter().all(|g| self.gate(*g).ok())
    }
    pub fn chk_ok(&self) -> bool {
        self.chk.iter().all(|c| *c)
    }
}

#[derive(Clone, Debug)]
pub struct DgLive {
    pub torq: i32,
    pub herr: f64,
    pub dwell: u32,
    pub unlocked: bool,
    pub tick: u64,
}

impl Default for DgLive {
    fn default() -> Self {
        DgLive { torq: 36, herr: 0.006, dwell: 0, unlocked: false, tick: 0 }
    }
}

// ------------------------------------------------------------------ live --

#[derive(Clone, Debug)]
pub struct Live {
    pub playing: bool,
    pub frame: u32,
    pub frames: u32,
    pub now: f64,
    pub reward: f64,
    pub falls: f64,
    pub kl: f64,
    pub lr: String,
    pub hist: Vec<f64>,
    pub now_hist: Vec<f64>,
}

// -------------------------------------------------------------- app-level --

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Home,
    Scenes,
    Robots,
    Train,
    Inspect,
    Validate,
    Runs,
    Deploy,
}

pub const MODES: [Mode; 8] = [
    Mode::Home,
    Mode::Scenes,
    Mode::Robots,
    Mode::Train,
    Mode::Inspect,
    Mode::Validate,
    Mode::Runs,
    Mode::Deploy,
];

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Home => "Home",
            Mode::Scenes => "Scenes",
            Mode::Robots => "Robots",
            Mode::Train => "Train",
            Mode::Inspect => "Inspect",
            Mode::Validate => "Validate",
            Mode::Runs => "Runs",
            Mode::Deploy => "Deploy",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Auto,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SweepState {
    Idle,
    Running,
    Aborted,
    Complete,
}

/// One reversible deletion, carried by a toast's Undo button.
#[derive(Clone, Debug)]
pub enum UndoAction {
    RestoreScene { scene: Scene, idx: usize },
    RestoreRecording { rec: Recording, idx: usize },
    RestoreRun { run: Run, idx: usize, took_from_kept: Vec<String> },
    RestoreTarget { target: Target, idx: usize },
    RestoreMember { set: String, idx: usize, member: String },
    RestoreCkpt { run: String, idx: usize, ck: Ckpt },
}

#[derive(Clone, Debug)]
pub struct Toast {
    pub text: String,
    pub undo: Option<UndoAction>,
    pub born: f64, // seconds since app start
}

#[derive(Clone, Debug, PartialEq)]
pub enum AppModal {
    None,
    Tour,
    DeleteScene,
    LastSceneBlocked,
    AddToSet,
    RenameSet,
    Preflight,
    Rerun,
    DeleteRun,
    PromoteCk { ck_id: String },
    RemoveRobotBlocked,
    Wizard { step: u32 },
    TargetForm { edit: Option<String> },
    RemoveTargetBlocked { held: String },
    Arm,
}

pub struct Sel {
    pub scene: Option<String>,
    pub set: Option<String>,
    pub robot: Option<String>,
    pub run: Option<String>,
    pub ck: Option<String>,
    pub ck_run: Option<String>,
    pub cell: Option<(usize, usize)>,
    pub dep: Option<String>,
    pub recipe: usize, // 0 = PD×friction, 1 = mass×push
}

pub const SWEEP_REF: [[Option<f64>; 8]; 5] = [
    [Some(0.81), Some(0.86), Some(0.90), Some(0.88), Some(0.84), Some(0.61), Some(0.55), Some(0.28)],
    [Some(0.77), Some(0.85), Some(0.92), Some(0.94), Some(0.89), Some(0.79), Some(0.58), Some(0.31)],
    [Some(0.64), Some(0.80), Some(0.88), Some(0.91), Some(0.86), Some(0.75), Some(0.52), Some(0.26)],
    [Some(0.33), Some(0.57), Some(0.76), Some(0.82), Some(0.78), Some(0.60), Some(0.35), Some(0.19)],
    [Some(0.11), Some(0.24), Some(0.51), Some(0.58), Some(0.54), Some(0.30), None, None],
];

pub struct Recipe {
    pub name: &'static str,
    pub fr: [f64; 5],
    pub ga: [f64; 8],
    pub xl: &'static str,
    pub yl: &'static str,
    pub xa: &'static str,
    pub xb: &'static str,
    pub ya: &'static str,
    pub yb: &'static str,
}

pub const RECIPES: [Recipe; 2] = [
    Recipe {
        name: "PD × friction",
        fr: [1.25, 1.00, 0.75, 0.50, 0.25],
        ga: [0.40, 0.57, 0.74, 0.91, 1.09, 1.26, 1.43, 1.60],
        xl: "X · PD gain",
        yl: "Y · friction",
        xa: "0.40×",
        xb: "1.60×",
        ya: "0.25",
        yb: "1.25",
    },
    Recipe {
        name: "mass × push",
        fr: [2.0, 1.6, 1.2, 0.8, 0.5],
        ga: [0.2, 0.4, 0.6, 0.8, 1.0, 1.2, 1.4, 1.6],
        xl: "X · push impulse",
        yl: "Y · mass scale",
        xa: "0.20",
        xb: "1.60",
        ya: "0.50×",
        yb: "2.00×",
    },
];

// ==========================================================================
// The store
// ==========================================================================

pub struct Store {
    pub mode: Mode,
    pub lang: usize, // 0 en, 1 zh
    pub theme: Theme,
    pub scenes: Vec<Scene>,
    pub sets: Vec<SceneSet>,
    pub robots: Vec<Robot>,
    pub runs: Vec<Run>,
    pub recordings: Vec<Recording>,
    pub targets: Vec<Target>,
    pub deploys: Vec<Deploy>,
    pub dg: Dg,
    pub dg_live: DgLive,
    pub sel: Sel,
    pub draft: Option<Scene>,
    pub dirty: bool,
    pub filter: String,
    pub live: Live,
    pub cell_phys: Option<(f64, f64)>, // (friction, gain) carried into Inspect
    pub sweep_grid: Option<[[Option<f64>; 8]; 5]>,
    pub sweep_state: SweepState,
    pub sweep_at: usize,
    pub sweep_ckpt: String,
    pub sweep_baseline: bool,
    pub toasts: Vec<Toast>,
    pub modal: AppModal,
    pub uid: u64,
    pub wiz_file: Option<String>,
    /// (hash, kb) of the newest real checkpoint on disk, when one exists.
    pub real_artifact: Option<(String, u64)>,
    pub clock: f64, // seconds since start, advanced by the app tick
    pub dep_serial: u32,
    pub ping_tick_counter: u64,
}

impl Default for Store {
    fn default() -> Self {
        Store::seed()
    }
}

impl Store {
    pub fn seed() -> Self {
        let mk = |id: &str, name: &str, f: &dyn Fn(&mut Scene)| {
            let mut s = Scene::base(id, name);
            f(&mut s);
            s
        };
        let scenes = vec![
            mk("s1", "warm-start", &|_| {}),
            mk("s2", "descend-70", &|s| s.stance = 0.70),
            mk("s3", "descend-62", &|s| s.stance = 0.62),
            mk("s4", "target-60", &|s| s.stance = 0.60),
            mk("s5", "slippery", &|s| s.friction = 0.35),
            mk("s6", "heavy", &|s| s.mass = 1.6),
            mk("s7", "shoved", &|s| s.push = 0.9),
            mk("s8", "baseline", &|_| {}),
            mk("s9", "stairs-probe", &|s| s.terrain = "step".into()),
        ];
        let sets = vec![
            SceneSet { id: "t1".into(), name: "crouch ladder".into(), members: vec!["s1".into(), "s2".into(), "s3".into(), "s4".into()] },
            SceneSet { id: "t2".into(), name: "robustness".into(), members: vec!["s5".into(), "s6".into(), "s7".into()] },
        ];
        let ladder_cfg = || {
            [("warm-start", 0.82), ("descend-70", 0.70), ("descend-62", 0.62), ("target-60", 0.60)]
                .iter()
                .map(|(n, st)| CfgScene {
                    name: (*n).into(),
                    stance: *st,
                    terrain: String::new(),
                    friction: 1.0,
                    kp: 1.0,
                    mass: 1.0,
                    push: 0.0,
                })
                .collect::<Vec<_>>()
        };
        let ladder = ["warm-start", "descend-70", "descend-62", "target-60"];
        let runs = vec![
            Run {
                id: "run3".into(), name: "crouch-v3".into(), set: "crouch ladder".into(),
                state: RunState::Running, archived: false, coll: false,
                steps: 2.10, best: Some(-0.260), stage: 2,
                stages: ladder.iter().map(|s| s.to_string()).collect(),
                round: 4, iter: 268, iters_per: 400,
                snapshot: Snapshot { scenes: 4, seed: "0xC0FFEE".into(), warm: None, cfg: ladder_cfg() },
                ckpts: vec![
                    Ckpt { id: "c4".into(), label: "ck-2100k".into(), score: -0.260, st: CkSt::Best, pname: None },
                    Ckpt { id: "c3".into(), label: "ck-1850k".into(), score: -0.288, st: CkSt::Ord, pname: None },
                    Ckpt { id: "c2".into(), label: "ck-1600k".into(), score: -0.334, st: CkSt::Ord, pname: None },
                    Ckpt { id: "c1".into(), label: "ck-1350k".into(), score: -0.361, st: CkSt::Ord, pname: None },
                ],
                diagnosis: None,
            },
            Run {
                id: "run2".into(), name: "crouch-v2".into(), set: "crouch ladder".into(),
                state: RunState::Completed, archived: false, coll: false,
                steps: 3.20, best: Some(-0.212), stage: 3,
                stages: ladder.iter().map(|s| s.to_string()).collect(),
                round: 6, iter: 400, iters_per: 400,
                snapshot: Snapshot { scenes: 4, seed: "0xC0FFEE".into(), warm: None, cfg: ladder_cfg() },
                ckpts: vec![
                    Ckpt { id: "c9".into(), label: "ck-3200k".into(), score: -0.212, st: CkSt::Prom, pname: Some("crouch-v2-final".into()) },
                    Ckpt { id: "c8".into(), label: "ck-2950k".into(), score: -0.231, st: CkSt::Ord, pname: None },
                ],
                diagnosis: None,
            },
            Run {
                id: "run1".into(), name: "flat-first".into(), set: "crouch ladder".into(),
                state: RunState::Failed, archived: false, coll: false,
                steps: 0.006, best: None, stage: 0,
                stages: vec!["warm-start".into()],
                round: 1, iter: 1, iters_per: 400,
                snapshot: Snapshot { scenes: 4, seed: "0xC0FFEE".into(), warm: None, cfg: ladder_cfg() },
                ckpts: vec![],
                diagnosis: Some("non-finite reward at iteration 0 — physics produced NaN before the first update".into()),
            },
            Run {
                id: "run0".into(), name: "stand-only".into(), set: "robustness".into(),
                state: RunState::Completed, archived: true, coll: false,
                steps: 1.00, best: Some(-0.41), stage: 0,
                stages: vec!["slippery".into(), "heavy".into(), "shoved".into()],
                round: 2, iter: 400, iters_per: 400,
                snapshot: Snapshot { scenes: 3, seed: "0x5EED".into(), warm: None, cfg: vec![] },
                ckpts: vec![Ckpt { id: "c0".into(), label: "ck-1000k".into(), score: -0.41, st: CkSt::Ord, pname: None }],
                diagnosis: None,
            },
        ];
        let mut hist = Vec::new();
        let mut now_hist = Vec::new();
        for i in 0..60 {
            let fi = i as f64;
            hist.push(-0.41 + fi * 0.0026 + (fi * 1.7).sin() * 0.004);
            now_hist.push(0.62 + (0.82 - 0.62) * (-fi / 16.0).exp() + (fi * 1.3).sin() * 0.004);
        }
        Store {
            mode: Mode::Home,
            lang: 0,
            theme: Theme::Auto,
            scenes,
            sets,
            robots: vec![Robot {
                id: "r1".into(), name: "Unitree G1-29dof".into(),
                links: 39, joints: 38, movable: 29, mapped: 12, meshes: 35,
                used_by: vec!["every scene".into()],
                urdf: {
                    let p = format!("{}/data/g1/g1.urdf", crate::nexus::repo_dir());
                    std::path::Path::new(&p).exists().then_some(p)
                },
            }],
            runs,
            recordings: vec![Recording { id: "rec1".into(), name: "descend-62-001".into(), scene: "descend-62".into(), frames: 301, resets: 4, dist: 1.62, path: None }],
            targets: vec![
                Target { id: "tg1".into(), name: "g1-lab-A".into(), model: "G1-29dof".into(), iface: "eth0".into(), ip: "192.168.123.161".into(), sdk: "unitree_sdk2 2.0".into(), cap: 70, rev: 0, ping: Ping::Unprobed },
                Target { id: "tg2".into(), name: "g1-field".into(), model: "G1-29dof".into(), iface: "wlan1".into(), ip: "10.0.4.77".into(), sdk: "unitree_sdk2 2.0".into(), cap: 50, rev: 0, ping: Ping::Unprobed },
            ],
            deploys: vec![Deploy {
                id: "d1".into(), name: "dep-001".into(), ck: "crouch-v2|ck-3200k".into(),
                pname: "crouch-v2-final".into(), target: "g1-lab-A".into(), tg_id: "tg1".into(), tg_rev: 0,
                hash: "77c1d40b".into(), state: DepState::Superseded, stage: 2,
                tg_snap: Some(TgSnap { model: "G1-29dof".into(), ip: "192.168.123.161".into(), iface: "eth0".into(), sdk: "unitree_sdk2 2.0".into(), cap: 70 }),
                gate_rep: vec![
                    Line::arg("onnx opset 17 · {0} KB · id {1}", vec!["407".into(), "77c1d40b".into()]),
                    Line::new("compat 5/5 · 1 accepted note"),
                    Line::new("sim2sim 24 eps · surv 97% · drift 5.9%"),
                    Line::new("dry-run · sat 8% · jerk p99 0.3"),
                ],
                chk: [true; 4], signer: "operator".into(), ts: "—".into(), pair: String::new(),
                note: Some(Line::new("superseded by a newer policy on this target")),
                events: vec![
                    Line::arg("armed at {0}% torque cap", vec!["40".into()]),
                    Line::arg("advanced to {0}% torque cap", vec!["70".into()]),
                    Line::arg("advanced to {0}% torque cap", vec!["100".into()]),
                    Line::new("rollout complete at 100%"),
                ],
            }],
            dg: Dg { target: Some("tg1".into()), ..Default::default() },
            dg_live: DgLive::default(),
            sel: Sel {
                scene: Some("s3".into()), set: Some("t1".into()), robot: Some("r1".into()),
                run: Some("run3".into()), ck: None, ck_run: None, cell: None, dep: None, recipe: 0,
            },
            draft: None,
            dirty: false,
            filter: String::new(),
            live: Live {
                playing: true, frame: 184, frames: 301, now: 0.641,
                reward: -0.2604, falls: 16.6, kl: 0.0103, lr: "7.6e-5".into(),
                hist, now_hist,
            },
            cell_phys: None,
            sweep_grid: None,
            sweep_state: SweepState::Idle,
            sweep_at: 0,
            sweep_ckpt: "ck-2100k".into(),
            sweep_baseline: true,
            toasts: vec![],
            modal: AppModal::None,
            uid: 100,
            wiz_file: None,
            real_artifact: None,
            clock: 0.0,
            dep_serial: 1,
            ping_tick_counter: 0,
        }
    }

    // ------------------------------------------------------------ helpers --

    pub fn nid(&mut self, p: &str) -> String {
        self.uid += 1;
        format!("{p}{}", self.uid)
    }

    pub fn scene(&self, id: &str) -> Option<&Scene> {
        self.scenes.iter().find(|s| s.id == id)
    }
    pub fn scene_mut(&mut self, id: &str) -> Option<&mut Scene> {
        self.scenes.iter_mut().find(|s| s.id == id)
    }
    pub fn set(&self, id: &str) -> Option<&SceneSet> {
        self.sets.iter().find(|s| s.id == id)
    }
    pub fn run(&self, id: &str) -> Option<&Run> {
        self.runs.iter().find(|r| r.id == id)
    }
    pub fn run_mut(&mut self, id: &str) -> Option<&mut Run> {
        self.runs.iter_mut().find(|r| r.id == id)
    }
    pub fn target(&self, id: &str) -> Option<&Target> {
        self.targets.iter().find(|t| t.id == id)
    }
    pub fn deploy(&self, id: &str) -> Option<&Deploy> {
        self.deploys.iter().find(|d| d.id == id)
    }

    /// The scene being viewed/edited: draft > selected > first > baseline.
    pub fn cur_scene(&self) -> Scene {
        if let Some(d) = &self.draft {
            return d.clone();
        }
        if let Some(id) = &self.sel.scene {
            if let Some(s) = self.scene(id) {
                return s.clone();
            }
        }
        self.scenes.first().cloned().unwrap_or_else(|| Scene::base("s0", "—"))
    }

    pub fn sets_of(&self, scene_id: &str) -> Vec<&SceneSet> {
        self.sets.iter().filter(|s| s.members.iter().any(|m| m == scene_id)).collect()
    }

    pub fn sweep_pass(&self) -> u32 {
        let Some(grid) = &self.sweep_grid else { return 0 };
        let m: Vec<f64> = grid.iter().flatten().take(self.sweep_at).filter_map(|v| *v).collect();
        if m.is_empty() {
            return 0;
        }
        (m.iter().filter(|v| **v > 0.5).count() as f64 / m.len() as f64 * 100.0).round() as u32
    }

    pub fn toast(&mut self, text: String) {
        let born = self.clock;
        self.toasts.push(Toast { text, undo: None, born });
    }
    pub fn toast_undo(&mut self, text: String, undo: UndoAction) {
        let born = self.clock;
        self.toasts.push(Toast { text, undo: Some(undo), born });
    }

    // -------------------------------------------------- certification core --

    /// The certification pair key: everything a gate result vouches for.
    pub fn pair_key(&self) -> String {
        let tg = self.dg.target.as_ref().and_then(|t| self.target(t));
        format!(
            "{}\u{a7}{}\u{a7}{}\u{a7}{}\u{a7}{}",
            self.dg.ck_run.clone().unwrap_or_default(),
            self.dg.ck.clone().unwrap_or_default(),
            self.dg.target.clone().unwrap_or_default(),
            tg.map(|t| t.rev as i64).unwrap_or(-1),
            self.dg.gen
        )
    }

    /// Reset gates + checklist and advance the generation so in-flight gate
    /// callbacks die. `msg` becomes a toast when present.
    pub fn dg_invalidate(&mut self, msg: Option<String>) {
        self.dg.gen += 1;
        self.dg.export = Gate::NotRun;
        self.dg.compat = Gate::NotRun;
        self.dg.sim2sim = Gate::NotRun;
        self.dg.dryrun = Gate::NotRun;
        self.dg.chk = [false; 4];
        if let Some(m) = msg {
            self.toast(m);
        }
    }

    /// FNV-1a-32 over UTF-8 bytes — the demo artifact fingerprint.
    pub fn fake_hash(s: &str) -> String {
        let mut h: u32 = 2166136261;
        for b in s.bytes() {
            h ^= b as u32;
            h = h.wrapping_mul(16777619);
        }
        format!("{h:08x}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv_matches_canonical_vector() {
        assert_eq!(Store::fake_hash("hello"), "4f9f2cab");
    }

    #[test]
    fn seed_shape() {
        let s = Store::seed();
        assert_eq!(s.scenes.len(), 9);
        assert_eq!(s.runs.len(), 4);
        assert_eq!(s.deploys.len(), 1);
        assert!(s.runs[3].archived);
        assert_eq!(s.runs[1].ckpts[0].st, CkSt::Prom);
    }

    #[test]
    fn pair_key_changes_with_gen_and_rev() {
        let mut s = Store::seed();
        let k0 = s.pair_key();
        s.dg_invalidate(None);
        assert_ne!(k0, s.pair_key());
        let k1 = s.pair_key();
        s.targets[0].rev += 1;
        assert_ne!(k1, s.pair_key());
    }

    #[test]
    fn sweep_pass_computes() {
        let mut s = Store::seed();
        s.sweep_grid = Some(SWEEP_REF);
        s.sweep_at = 40;
        assert_eq!(s.sweep_pass(), 76);
    }
}
