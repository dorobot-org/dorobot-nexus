//! The modal host: one overlay panel whose generic children are arranged per
//! AppModal kind — every dialog, the six-step import wizard, and the tour.

use super::*;

fn modal_key(m: &AppModal) -> String {
    format!("{m:?}")
}

pub fn sync_modal(app: &mut App, cx: &mut Cx) {
    let l = app.light();
    let lang = app.st.lang;
    let tt = |k: &str| tr(lang, k).to_string();
    let tf = |k: &str, a: &[&str]| trf(lang, k, a);
    let host = app.ui.widget(cx, ids!(modal_host));
    if host.is_empty() {
        return;
    }
    let vis = app.st.modal != AppModal::None;
    app.ui.view(cx, ids!(modal_host)).set_visible(cx, vis);
    if !vis {
        app.modal_seen = String::new();
        return;
    }
    let key = if app.st.modal == AppModal::TaskEditor {
        format!(
            "TaskEditor/{:?}",
            app.st.task_draft.as_ref().map(|d| &d.page)
        )
    } else {
        modal_key(&app.st.modal)
    };
    let fresh = app.modal_seen != key;
    app.modal_seen = key;
    {
        let width = (app.window_width - 32.).clamp(240., 560.);
        let mut p = host.widget(cx, ids!(panel));
        script_apply_eval!(cx, p, { width: #(width), draw_bg +: { light: #(l as f64) } });
    }
    // hide everything, then reveal per kind
    let hide_paths: [&[LiveId]; 26] = [
        ids!(panel.sub),
        ids!(panel.blast),
        ids!(panel.wsteps),
        ids!(panel.info),
        ids!(panel.in_cap0),
        ids!(panel.in_wrap0),
        ids!(panel.in_cap1),
        ids!(panel.in_wrap1),
        ids!(panel.in_cap2),
        ids!(panel.in_wrap2),
        ids!(panel.opt0),
        ids!(panel.opt1),
        ids!(panel.opt2),
        ids!(panel.opt3),
        ids!(panel.opt4),
        ids!(panel.opt5),
        ids!(panel.opt6),
        ids!(panel.opt7),
        ids!(panel.chk0),
        ids!(panel.chk1),
        ids!(panel.chk2),
        ids!(panel.chk3),
        ids!(panel.chk4),
        ids!(panel.foot.b_back),
        ids!(panel.foot.b_ok),
        ids!(panel.foot.b_danger),
    ];
    for p in hide_paths {
        let w = host.widget(cx, p);
        if !w.is_empty() {
            w.set_visible(cx, false);
        }
    }
    let title;
    let mut sub: Option<String> = None;
    let mut blast: Option<(f64, String)> = None;
    let mut wsteps: Option<String> = None;
    let mut info: Option<String> = None;
    let mut inputs: Vec<(String, String)> = vec![]; // (cap, default)
    let mut opts: Vec<(String, bool)> = vec![];
    let mut chks: Vec<(f64, String, String)> = vec![];
    let mut ok: Option<String> = None;
    let mut danger: Option<String> = None;
    let mut back = false;
    let st = &app.st;
    match &st.modal {
        AppModal::None => return,
        AppModal::TaskEditor => {
            match st
                .task_draft
                .as_ref()
                .ok_or("No task draft".into())
                .and_then(crate::task_editor::view)
            {
                Ok(v) => {
                    title = v.title;
                    info = Some(v.info);
                    inputs = v.inputs;
                    opts = v.options;
                    ok = v.ok;
                    danger = v.danger;
                    back = v.back;
                }
                Err(e) => {
                    title = "Task editor".into();
                    info = Some(e);
                }
            }
        }
        AppModal::Tour => {
            title = "Nexus Studio guide".into();
            sub = Some("GPU training, measured simulation and retained policy evidence.".into());
            opts=vec![
                ("Home — training progress and the selected run's reward. Rewards from different tasks are not combined.".into(),false),
                ("Scenes — author and save scene sets for the native Unitree trainer. Robot tasks carry their own simulator settings.".into(),false),
                ("Robots — import, edit calibration and task settings, or transfer a portable robot bundle.".into(),false),
                ("Train — launch a GPU job, continue a checkpoint, manage the queue, or select an SSH training host.".into(),false),
                ("Inspect — review recorded channels and annotations, or start policy physics with velocity and force controls.".into(),false),
                ("Validate — evaluate held-out commands and seeds. Compare two compatible checkpoints with synchronized replays and export the report.".into(),false),
                ("Runs — inspect captured recipes and checkpoints; export, reproduce, retain, clean up and restore artifacts.".into(),false),
                ("Deploy — connect a robot agent, prepare and dry-run a policy, then arm and advance command stages. The target watchdog stops expired sessions.".into(),false),
            ];
            info=Some("Simulator evidence verifies the software workflow. Physical walking performance is established on a connected, calibrated robot.".into());
            ok = Some("Start exploring".into());
        }
        AppModal::DeleteScene => {
            let name = st
                .sel
                .scene
                .as_ref()
                .and_then(|id| st.scene(id))
                .map(|s| s.name.clone())
                .unwrap_or_default();
            title = tf("Delete {0}?", &[&name]);
            sub = Some(tt("Blast radius, before you decide:"));
            let sets = st
                .sel
                .scene
                .as_ref()
                .map(|id| st.sets_of(id))
                .unwrap_or_default();
            let inset = if sets.is_empty() {
                tt("In no sets.")
            } else {
                tf(
                    "In set {0} — the set will show a broken slot until fixed.",
                    &[&sets
                        .iter()
                        .map(|s| s.name.clone())
                        .collect::<Vec<_>>()
                        .join(", ")],
                )
            };
            blast = Some((1.0, format!("{inset} {}", tt("Runs are unaffected — every run holds its own snapshot. Recordings keep their tags."))));
            danger = Some(tt("Delete"));
        }
        AppModal::LastSceneBlocked => {
            title = tt("Delete?");
            blast = Some((2.0, tt("Blocked: this is the last scene. An empty library has no baseline to simulate — duplicate first, then delete.")));
            ok = Some(tt("Understood"));
        }
        AppModal::AddToSet => {
            let name = st
                .sel
                .scene
                .as_ref()
                .and_then(|id| st.scene(id))
                .map(|s| s.name.clone())
                .unwrap_or_default();
            title = tf("Add {0} to a set", &[&name]);
            sub = Some(tt(
                "A scene can belong to several sets; a set is one ordering of scenes.",
            ));
            for s in &st.sets {
                opts.push((
                    format!(
                        "{} · {}",
                        s.name,
                        tf("{0} scenes", &[&s.members.len().to_string()])
                    ),
                    false,
                ));
            }
        }
        AppModal::QueueSettings { page, draft } => {
            title = format!("Training launch settings · {} / 4", page + 1);
            sub=Some("Running jobs continue when Studio closes; keep Studio open to dispatch queued jobs. Parallel PPO uses CPU or one CUDA GPU per learner on the selected host. Apple MPS uses one learner.".into());
            let c = draft;
            inputs = match page {
                0 => vec![
                    ("Execution: local or ssh".into(), c.transport.clone()),
                    (
                        "Delay before launch (minutes)".into(),
                        c.delay_minutes.to_string(),
                    ),
                    ("SSH host (user@host or alias)".into(), c.host.clone()),
                ],
                1 => vec![
                    ("Remote Nexus repository".into(), c.remote_repo.clone()),
                    ("Remote run storage directory".into(), c.remote_root.clone()),
                    (
                        "Remote nexus-studio executable".into(),
                        c.studio_binary.clone(),
                    ),
                ],
                2 => vec![
                    ("Remote Python executable".into(), c.python.clone()),
                    ("SSH port".into(), c.port.to_string()),
                    ("Private key file (optional)".into(), c.identity.clone()),
                ],
                _ => vec![
                    (
                        "Known hosts file (optional; default SSH config)".into(),
                        c.known_hosts.clone(),
                    ),
                    (
                        "Learner processes on this host (MPS: 1)".into(),
                        c.processes.to_string(),
                    ),
                ],
            };
            back = *page > 0;
            ok = Some(if *page < 3 { "Next" } else { "Save settings" }.into());
        }
        AppModal::AgentArm => {
            title = "Arm prepared policy".into();
            sub=Some("Stage 0 runs the policy with zero velocity command. Later stages raise command limits after measured dwell time.".into());
            info = Some(
                if app
                    .deployment
                    .session
                    .as_ref()
                    .is_some_and(|s| s.status["agent"]["simulation"] == true)
                {
                    "This target is a physics simulator. No physical robot is connected.".into()
                } else {
                    "Arming enables physical robot motion. Support the robot for initial testing. The agent stops on connection loss; torque-off stops on G1 and Open Duck require support.".into()
                },
            );
            ok = Some("Arm policy".into());
        }
        AppModal::AgentCommand => {
            title = "Command target velocity".into();
            sub = Some(
                "Commands are bounded by the current stage and the target's configured limits."
                    .into(),
            );
            inputs.push((
                "Forward, lateral (m/s), yaw (rad/s)".into(),
                "0, 0, 0".into(),
            ));
            ok = Some("Apply".into());
        }
        AppModal::Evaluation => {
            title = "Evaluate / compare checkpoints".into();
            sub=Some("Same held-out commands and seeds for each policy. A blank comparison path evaluates only the selected checkpoint. Each trial ends at its first fall or task limit.".into());
            inputs = vec![
                (
                    "Comparison checkpoint path (optional)".into(),
                    String::new(),
                ),
                ("Seconds per trial (0.2–30)".into(), "10".into()),
                (
                    "Held-out seeds, comma separated".into(),
                    "10001,20003,30011".into(),
                ),
            ];
            ok = Some("Run evaluation".into());
        }
        AppModal::EvaluationExport => {
            title = "Export evaluation report".into();
            sub = Some(
                "Export JSON, Markdown, CSV and measured trajectories to a new folder.".into(),
            );
            inputs = vec![("New absolute output folder".into(), String::new())];
            ok = Some("Export report".into());
        }
        AppModal::SimulationImport => {
            title = "Recover live recording".into();
            sub = Some("Choose a simulation session folder containing status.json and simulation-motion.jsonl. Complete recorded steps survive closing Studio, even before Save and review.".into());
            let root = std::path::PathBuf::from(crate::nexus::repo_dir()).join("simulations");
            let mut folders = std::fs::read_dir(&root)
                .ok()
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.join("simulation-motion.jsonl").is_file())
                .collect::<Vec<_>>();
            folders.sort();
            inputs = vec![(
                "Session folder".into(),
                folders.last().unwrap_or(&root).to_string_lossy().into(),
            )];
            ok = Some("Recover and review".into());
        }
        AppModal::SimulationCommand => {
            title = "Command body velocity".into();
            sub = Some("Forward and lateral velocities are in the robot frame. Applied on the next physics step. Values outside the training command range test generalization.".into());
            let command = app.simulation.as_ref().map(|s| &s.status["command"]);
            inputs = ["Forward (m/s)", "Lateral (m/s)", "Yaw (rad/s)"]
                .iter()
                .enumerate()
                .map(|(i, label)| {
                    (
                        label.to_string(),
                        command
                            .and_then(|v| v[i].as_f64())
                            .unwrap_or(0.)
                            .to_string(),
                    )
                })
                .collect();
            ok = Some("Apply command".into());
        }
        AppModal::SimulationPush => {
            title = "Push robot base".into();
            sub = Some("World-frame horizontal force, held for the requested simulation time rounded up to a control step. A paused simulation applies it when stepped or resumed.".into());
            inputs = vec![
                ("World X force (N)".into(), "1".into()),
                ("World Y force (N)".into(), "0".into()),
                ("Duration (seconds)".into(), "0.1".into()),
            ];
            ok = Some("Apply push".into());
        }
        AppModal::ReviewEdit => {
            title = "Edit episode".into();
            if let Some(segment) = app
                .review
                .document
                .as_ref()
                .and_then(|d| d.segments.get(app.review.segment))
            {
                sub=Some(format!("Original frames {}..{}. Keep range excludes the end frame. Source frames remain recoverable.",segment.start,segment.end));
                inputs = vec![
                    ("Keep from frame".into(), segment.keep_start.to_string()),
                    (
                        "Keep until frame (excluded)".into(),
                        segment.keep_end.to_string(),
                    ),
                    ("Annotation".into(), segment.annotation.clone()),
                ];
                ok = Some("Save episode".into());
            }
        }
        AppModal::ReviewExport => {
            title = "Export edited recording".into();
            sub=Some("Creates recording.json with included frames and telemetry, plus provenance.json mapping every exported frame to its source. Existing folders are preserved.".into());
            inputs = vec![("New absolute output folder".into(), String::new())];
            ok = Some("Export dataset".into());
        }
        AppModal::ImportRobot { path, summary } => {
            title = "Import robot".into();
            if let Some(summary) = summary {
                sub = Some(path.clone());
                info = Some(summary.clone());
                back = true;
                ok = Some("Import to library".into());
            } else {
                inputs.push((
                    "URDF file or bundle.json / bundle directory".into(),
                    path.clone(),
                ));
                opts.push(("Browse files…".into(), false));
                info = Some("Review the robot structure, all referenced resources and any saved task. Import creates a separate library identity.".into());
                ok = Some("Review import".into());
            }
        }
        AppModal::ExportRobot => {
            title = "Export robot bundle".into();
            let robot = st
                .sel
                .robot
                .as_ref()
                .and_then(|id| st.robots.iter().find(|r| &r.id == id));
            sub = robot.map(|r| r.name.clone());
            let default = std::path::PathBuf::from(crate::nexus::repo_dir())
                .join("exports")
                .join(format!(
                    "robot-{}",
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                ));
            inputs.push((
                "New export directory".into(),
                default.to_string_lossy().into(),
            ));
            info = Some("Copies geometry, materials, textures, available license files and the saved task revision. Checksums verify every asset when the bundle is imported on another workspace. Training dependencies are installed separately.".into());
            ok = Some("Export bundle".into());
        }
        AppModal::RenameRobot => {
            title = tt("Rename robot");
            inputs.push((
                tt("Name"),
                st.sel
                    .robot
                    .as_ref()
                    .and_then(|id| st.robots.iter().find(|r| &r.id == id))
                    .map(|r| r.name.clone())
                    .unwrap_or_default(),
            ));
            info = Some(tt("Display name only. Robot ID, simulator model and existing training/checkpoint contracts stay the same."));
            ok = Some(tt("Save"));
        }
        AppModal::RemoveRobot => {
            title = tt("Remove robot");
            let refs = st
                .sel
                .robot
                .as_ref()
                .map(|id| st.robot_references(id))
                .unwrap_or_default();
            if refs.is_empty() {
                info = Some("Preview removal of this unused robot and its owned assets. Workspace trash restores both the robot settings and files, including after restart.".into());
                ok = Some("Preview files".into());
            } else {
                blast = Some((2.0, format!("Still used by {}", refs.join(", "))));
            }
        }
        AppModal::ContinueRun => {
            title = tt("Continue from checkpoint");
            let run = st.sel.run.as_ref().and_then(|id| st.run(id));
            inputs.push((
                tt("New run name"),
                run.map(|r| format!("{} continuation", r.name))
                    .unwrap_or_default(),
            ));
            inputs.push((tt("Additional iterations"), "1000".into()));
            inputs.push((
                tt("Checkpoint path"),
                crate::nexus::selected_artifact(st).unwrap_or_default(),
            ));
            info = Some(tt("A new run starts from these weights using the saved robot task. Duck PPO also restores optimizer and normalization state. G1/CPU warm starts initialize a new optimizer."));
            if app.real_proc.is_none() {
                ok = Some(tt("Continue training"));
            }
        }
        AppModal::RenameSet => {
            title = tt("Rename set");
            let cur = st
                .sel
                .set
                .as_ref()
                .and_then(|id| st.set(id))
                .map(|s| s.name.clone())
                .unwrap_or_default();
            inputs.push((tt("Name"), cur));
            ok = Some(tt("Rename"));
        }
        AppModal::Preflight => {
            let set = st.sel.set.as_ref().and_then(|id| st.set(id));
            let set_name = set.map(|s| s.name.clone()).unwrap_or_default();
            let n = set.map(|s| s.members.len()).unwrap_or(0);
            let broken = set
                .map(|s| s.members.iter().any(|m| st.scene(m).is_none()))
                .unwrap_or(false);
            title = tf("Pre-flight · {0}", &[&set_name]);
            sub = Some(tt(
                "A run snapshots its inputs at launch. Edits after launch apply to the next run.",
            ));
            if broken || n == 0 {
                blast = Some((2.0, tt("This set has a broken slot (a deleted scene). Fix or remove it in the composer — training through a hole is refused.")));
            }
            inputs.push((tt("Run name"), "crouch-v4".into()));
            opts = vec![(tt("Fresh"), true)];
            let iters = n * 400 * 6;
            let robot = st
                .sel
                .robot
                .as_ref()
                .and_then(|id| st.robots.iter().find(|r| &r.id == id));
            let task = robot.and_then(|r| crate::nexus::robot_training_task(r).ok().flatten());
            info = Some(if let Some(task) = task {
                format!("{task}: uses the selected robot's trainer profile, including its environment budget and resume checkpoint. G1 scene settings do not apply.")
            } else if std::env::var("NEXUS_BACKEND").as_deref() == Ok("cpu") {
                "CPU cart-pole demonstration: 256 environments, 2M steps. G1 terrain and stance settings do not apply to this task.".into()
            } else {
                format!("Unitree G1 / Zealot: {n} scenes × 400 iterations × 6 rounds = {iters} iterations; 256 environments. Scene settings are snapshotted at launch.")
            });
            if !broken && n > 0 {
                ok = Some(tt("Launch"));
            }
        }
        AppModal::Rerun => {
            let r = st.sel.run.as_ref().and_then(|id| st.run(id));
            let name = r.map(|r| r.name.clone()).unwrap_or_default();
            title = tf("Re-run {0}", &[&name]);
            sub = Some(tt("Launches from this run’s snapshot — identical fields and seed, regardless of what the library has become."));
            if let Some(r) = r {
                info = Some(tf(
                    "Snapshot: {0} scenes · seed {1}",
                    &[&r.snapshot.scenes.to_string(), &r.snapshot.seed],
                ));
            }
            inputs.push((tt("New run name"), format!("{name}-repro")));
            ok = Some(tt("Launch"));
        }
        AppModal::ExportPolicy { checkpoint } => {
            title = "Export normalized ONNX policy".into();
            sub = Some(checkpoint.clone());
            let name = format!(
                "policy-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            );
            let destination = std::path::PathBuf::from(crate::nexus::repo_dir())
                .join("exports")
                .join(name);
            inputs.push((
                "New export directory".into(),
                destination.to_string_lossy().into(),
            ));
            info = Some("Exports the selected weights, includes the saved observation normalizer, and checks ONNX actions against the source policy. The bundle contains source weights, ONNX, provenance and controller requirements.".into());
            if app.export_job.is_none() {
                ok = Some("Export and verify".into());
            }
        }
        AppModal::ArtifactRetention => {
            title = "Checkpoint retention".into();
            sub = Some("Keep the newest checkpoints, best measured reward, promoted policies, final export and saved recipe references. Review the exact file plan before applying it.".into());
            inputs.push((
                "Newest checkpoints to keep".into(),
                st.retention_keep.to_string(),
            ));
            ok = Some("Preview cleanup".into());
        }
        AppModal::ArtifactCleanup { page } => {
            title = "Review artifact cleanup".into();
            if let Some(plan) = &app.artifact_plan {
                let lines = plan
                    .description()
                    .lines()
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                let pages = lines.len().div_ceil(5);
                let page = (*page).min(pages.saturating_sub(1));
                sub = Some(format!(
                    "{} · page {}/{}",
                    plan.entry.label,
                    page + 1,
                    pages
                ));
                info = Some(
                    lines
                        .iter()
                        .skip(page * 5)
                        .take(5)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("\n\n"),
                );
                back = page > 0;
                if page + 1 < pages {
                    ok = Some("Next files".into());
                } else {
                    danger = Some("Move to trash".into());
                }
            }
        }
        AppModal::Trash { page } => {
            title = "Workspace trash".into();
            sub = Some("Choose a deletion to restore its files and ledger entries. Trash is retained across restarts.".into());
            opts = st
                .trash
                .iter()
                .rev()
                .skip(page * 5)
                .take(5)
                .map(|e| (format!("{} · {} files", e.label, e.files.len()), false))
                .collect();
            info = Some(format!(
                "{} saved deletions · page {}",
                st.trash.len(),
                page + 1
            ));
            back = *page > 0;
            ok = Some(
                if (page + 1) * 5 < st.trash.len() {
                    "Next page"
                } else {
                    "Close"
                }
                .into(),
            );
        }
        AppModal::RestoreTrash { id } => {
            title = "Restore deleted artifacts?".into();
            if let Some(entry) = st.trash.iter().find(|e| &e.id == id) {
                sub = Some(entry.label.clone());
                info = Some(format!("Restore {} files ({:.2} MiB) and their ledger entries. Existing files are never overwritten.", entry.files.len(), entry.files.iter().map(|f| f.bytes).sum::<u64>() as f64 / 1048576.0));
                ok = Some("Restore".into());
                danger = Some("Permanently delete…".into());
            }
        }
        AppModal::PurgeTrash { id } => {
            title = "Permanently delete trashed files?".into();
            if let Some(entry) = st.trash.iter().find(|e| &e.id == id) {
                sub = Some(entry.label.clone());
                blast = Some((2.0, format!("Delete {} files ({:.2} MiB) permanently. This frees the disk space and removes this restore point. This action cannot be undone.", entry.files.len(), entry.files.iter().map(|f| f.bytes).sum::<u64>() as f64 / 1048576.0)));
                danger = Some("Delete permanently".into());
            }
        }
        AppModal::DeleteRun => {
            let r = st.sel.run.as_ref().and_then(|id| st.run(id));
            let name = r
                .map(|r| {
                    if r.coll {
                        tt("(kept checkpoints)")
                    } else {
                        r.name.clone()
                    }
                })
                .unwrap_or_default();
            title = tf("Delete run {0}?", &[&name]);
            if let Some(r) = r {
                let prom = if r.coll {
                    0
                } else {
                    r.ckpts.iter().filter(|c| c.st == CkSt::Prom).count()
                };
                let promtxt = if prom > 0 {
                    tf(" — {0} promoted.", &[&prom.to_string()])
                } else {
                    ". ".into()
                };
                blast = Some((1.0, tf("This run owns {0} checkpoints{1} Recordings survive; they carry their own tags.", &[&r.ckpts.len().to_string(), &promtxt])));
                if prom > 0 {
                    opts = vec![
                        (tt("Keep the promoted checkpoints"), app.keep_sel == 0),
                        (tt("Remove from ledger (files retained)"), app.keep_sel == 1),
                    ];
                }
            }
            danger = Some(tt("Delete run"));
        }
        AppModal::PromoteCk { .. } => {
            title = tt("Promote checkpoint");
            sub = Some(tt("A promoted checkpoint is exempt from pruning and can be deployed or used as a warm start."));
            inputs.push((tt("Name"), "crouch-v3-final".into()));
            ok = Some(tt("Promote"));
        }
        AppModal::RemoveRobotBlocked => {
            let r = st
                .sel
                .robot
                .as_ref()
                .and_then(|id| st.robots.iter().find(|r| &r.id == id));
            title = tf(
                "Remove {0}?",
                &[&r.map(|r| r.name.clone()).unwrap_or_default()],
            );
            let used = r
                .map(|r| {
                    r.used_by
                        .iter()
                        .map(|u| tt(u))
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .unwrap_or_default();
            blast = Some((2.0, tf("Blocked: referenced by {0}. Point those scenes at another robot first — nothing is greyed out without its reason.", &[&used])));
            ok = Some(tt("Understood"));
        }
        AppModal::Wizard { step } => {
            title = tt("Import robot");
            let names = ["Source", "Assets", "Structure", "Rehearse", "Map", "Commit"];
            let marks: Vec<String> = names
                .iter()
                .enumerate()
                .map(|(i, n)| {
                    let i = i as u32 + 1;
                    if i < *step {
                        format!("✓ {}", tt(n))
                    } else if i == *step {
                        format!("[{}]", tt(n))
                    } else {
                        tt(n)
                    }
                })
                .collect();
            wsteps = Some(marks.join("  ·  "));
            match step {
                1 => {
                    sub = Some(tt(
                        "Pick the robot description. The wizard copies everything on commit.",
                    ));
                    info = Some(match &st.wiz_file {
                        Some(p) => p.clone(),
                        None => tt("No file chosen yet — Choose file opens the system browser."),
                    });
                    ok = Some(if st.wiz_file.is_some() {
                        tt("Continue")
                    } else {
                        tt("Choose file…")
                    });
                }
                _ => {
                    sub = Some("Validate the selected URDF and copy its referenced assets into the robot library.".into());
                    match st
                        .wiz_file
                        .as_ref()
                        .map(|p| crate::robot_import::inspect(std::path::Path::new(p)))
                    {
                        Some(Ok(report)) => {
                            info = Some(report);
                            ok = Some("Import robot".into());
                        }
                        Some(Err(e)) => {
                            blast = Some((2.0, e));
                        }
                        None => {
                            info = Some("Choose a URDF first.".into());
                        }
                    }
                    back = true;
                }
            }
        }
        AppModal::TargetForm { edit } if !st.demo => {
            let target = edit.as_ref().and_then(|id| st.target(id));
            title = if target.is_some() {
                "Edit robot target".into()
            } else {
                "Add robot target".into()
            };
            sub=Some("Use sim://local for a simulator agent, or a robot agent reached through SSH forwarding or HTTPS. The selected robot defines the model contract.".into());
            inputs.push((
                "Name".into(),
                target
                    .map(|t| t.name.clone())
                    .unwrap_or_else(|| "Local simulator".into()),
            ));
            inputs.push((
                "Agent endpoint".into(),
                target
                    .filter(|t| t.sdk == "nexus-agent/1")
                    .map(|t| t.ip.clone())
                    .unwrap_or_else(|| "sim://local".into()),
            ));
            inputs.push((
                "Token file (remote target)".into(),
                target
                    .filter(|t| t.sdk == "nexus-agent/1")
                    .map(|t| t.iface.clone())
                    .unwrap_or_default(),
            ));
            ok = Some("Save target".into());
        }
        AppModal::TargetForm { edit } => {
            let editing = edit.as_ref().and_then(|id| st.target(id));
            title = if editing.is_some() {
                tt("Edit target")
            } else {
                tt("Add target")
            };
            if editing.is_none() {
                sub = Some(tt(
                    "A target is an authored endpoint — deployments reference it by snapshot.",
                ));
            }
            let (n, ip, capv) = match editing {
                Some(t) => (
                    t.name.clone(),
                    format!("{} · {}", t.ip, t.iface),
                    t.cap.to_string(),
                ),
                None => (
                    "g1-lab-C".into(),
                    "192.0.2.162 · eth0".into(),
                    "70".into(),
                ),
            };
            inputs.push((tt("Name"), n));
            inputs.push((tt("IP · interface"), ip));
            inputs.push((tt("Torque cap profile"), capv));
            ok = Some(if editing.is_some() {
                tt("Save")
            } else {
                tt("Add")
            });
        }
        AppModal::RemoveTargetBlocked { held } => {
            title = tt("Remove?");
            blast = Some((2.0, tf("Blocked: {0} is live on this target. Retire or roll back the deployment first — nothing is greyed out without its reason.", &[held])));
            ok = Some(tt("Understood"));
        }
        AppModal::Arm => {
            let tg = st.dg.target.as_ref().and_then(|t| st.target(t));
            title = tf(
                "Arm rollout on {0}?",
                &[&tg.map(|t| t.name.clone()).unwrap_or_default()],
            );
            sub = Some(tt(
                "Arming snapshots everything below into an immutable record.",
            ));
            if let (Some(tg), Gate::Pass { hash, .. }) = (tg, &st.dg.export) {
                info = Some(tf("{0} · {1} · id {2} → {3} ({4}) · torque staged 40 → 70 → 100% of the {5}% profile", &[
                    &st.dg.pname.clone().unwrap_or_default(),
                    &st.dg.ck.clone().unwrap_or_default(),
                    hash,
                    &tg.name,
                    &tg.ip,
                    &tg.cap.to_string(),
                ]));
            }
            ok = Some(tt("Arm at 40%"));
        }
    }
    set_label(cx, &host, ids!(panel.title), &title, l);
    if let Some(s) = &sub {
        host.widget(cx, ids!(panel.sub)).set_visible(cx, true);
        set_label(cx, &host, ids!(panel.sub), s, l);
    }
    if let Some((tone, text)) = &blast {
        host.widget(cx, ids!(panel.blast)).set_visible(cx, true);
        set_label(cx, &host, ids!(panel.blast.lbl), text, l);
        let mut b = host.widget(cx, ids!(panel.blast));
        let tf2 = *tone;
        script_apply_eval!(cx, b, { draw_bg +: { tone: #(tf2) light: #(l as f64) } });
    }
    if let Some(w) = &wsteps {
        host.widget(cx, ids!(panel.wsteps)).set_visible(cx, true);
        set_label(cx, &host, ids!(panel.wsteps), w, l);
    }
    if let Some(i) = &info {
        host.widget(cx, ids!(panel.info)).set_visible(cx, true);
        set_label(cx, &host, ids!(panel.info), i, l);
    }
    let icaps = [
        ids!(panel.in_cap0),
        ids!(panel.in_cap1),
        ids!(panel.in_cap2),
    ];
    // The wrapper carries visibility, the inner TextInput carries the text.
    let iwraps = [
        ids!(panel.in_wrap0),
        ids!(panel.in_wrap1),
        ids!(panel.in_wrap2),
    ];
    let ifields = [
        ids!(panel.in_wrap0.input0),
        ids!(panel.in_wrap1.input1),
        ids!(panel.in_wrap2.input2),
    ];
    for (i, (capt, default)) in inputs.iter().enumerate() {
        host.widget(cx, icaps[i]).set_visible(cx, true);
        set_label(cx, &host, icaps[i], &caps(capt), l);
        host.widget(cx, iwraps[i]).set_visible(cx, true);
        if fresh {
            host.widget(cx, ifields[i]).set_text(cx, default);
        }
    }
    let opaths = [
        ids!(panel.opt0),
        ids!(panel.opt1),
        ids!(panel.opt2),
        ids!(panel.opt3),
        ids!(panel.opt4),
        ids!(panel.opt5),
        ids!(panel.opt6),
        ids!(panel.opt7),
    ];
    for (i, (label, on)) in opts.iter().enumerate() {
        if i >= opaths.len() {
            break;
        }
        let row = host.widget(cx, opaths[i]);
        row.set_visible(cx, true);
        set_label(cx, &row, ids!(nm), label, l);
        set_label(cx, &row, ids!(tg), "", l);
        row.widget(cx, ids!(tg)).set_visible(cx, false);
        let onf = if *on { 1.0 } else { 0.0 };
        let mut rw = host.widget(cx, opaths[i]);
        script_apply_eval!(cx, rw, { draw_bg +: { on: #(onf) light: #(l as f64) } });
        let pw = row.widget(cx, ids!(pill));
        if !pw.is_empty() {
            pw.set_visible(cx, false);
        }
    }
    let cpaths = [
        ids!(panel.chk0),
        ids!(panel.chk1),
        ids!(panel.chk2),
        ids!(panel.chk3),
        ids!(panel.chk4),
    ];
    for (i, (stv, nm, ds)) in chks.iter().enumerate() {
        if i >= cpaths.len() {
            break;
        }
        let row = host.widget(cx, cpaths[i]);
        row.set_visible(cx, true);
        let ic = match *stv as i32 {
            0 => "✓",
            1 => "!",
            2 => "✕",
            _ => "·",
        };
        set_label(cx, &row, ids!(ico), ic, l);
        {
            let mut iw = row.widget(cx, ids!(ico));
            let sv = *stv;
            script_apply_eval!(cx, iw, { draw_text +: { st: #(sv) } });
        }
        set_label(cx, &row, ids!(body.nm), nm, l);
        set_label(cx, &row, ids!(body.ds), ds, l);
        for bp in [ids!(act), ids!(act2)] {
            let b = row.button(cx, bp);
            if !b.is_empty() {
                b.set_visible(cx, false);
            }
        }
    }
    set_btn(
        cx,
        &host,
        ids!(panel.foot.b_cancel),
        &tt("Cancel"),
        true,
        false,
        true,
        l,
    );
    if back {
        set_btn(
            cx,
            &host,
            ids!(panel.foot.b_back),
            &tt("Back"),
            true,
            false,
            true,
            l,
        );
    }
    if let Some(o) = &ok {
        set_btn(cx, &host, ids!(panel.foot.b_ok), o, true, true, true, l);
    }
    if let Some(d) = &danger {
        set_btn(
            cx,
            &host,
            ids!(panel.foot.b_danger),
            d,
            true,
            false,
            true,
            l,
        );
    }
}

/// Modal option-row activation (driver + click share this).
pub fn modal_opt(app: &mut App, cx: &mut Cx, i: usize) {
    if app.st.modal == AppModal::TaskEditor {
        if app.can_edit() {
            if let Err(e) = crate::task_editor::option(&mut app.st, i) {
                app.st.toast(e);
            }
        }
        app.sync_all(cx);
        return;
    }
    match &app.st.modal {
        AppModal::ImportRobot { summary: None, .. } if i == 0 => {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Robot or bundle", &["urdf", "json"])
                .pick_file()
            {
                app.st.modal = AppModal::ImportRobot {
                    path: path.to_string_lossy().into(),
                    summary: None,
                };
            }
        }
        AppModal::AddToSet => {
            if let Some(s) = app.st.sets.get(i) {
                let id = s.id.clone();
                app.st.addto_set_yes(&id);
            }
        }
        AppModal::Trash { page } => {
            if let Some(entry) = app.st.trash.iter().rev().nth(page * 5 + i) {
                app.st.modal = AppModal::RestoreTrash {
                    id: entry.id.clone(),
                };
            }
        }
        AppModal::Preflight => app.warm_sel = i.min(2),
        AppModal::DeleteRun => app.keep_sel = i.min(1),
        _ => {}
    }
    app.sync_all(cx);
}

pub fn modal_back(app: &mut App, cx: &mut Cx) {
    if app.st.modal == AppModal::TaskEditor {
        if app.can_edit() {
            if let Err(e) = crate::task_editor::back(&mut app.st) {
                app.st.toast(e);
            }
        }
        app.sync_all(cx);
        return;
    }
    match app.st.modal.clone() {
        AppModal::QueueSettings { page, draft } => {
            app.st.modal = AppModal::QueueSettings {
                page: page.saturating_sub(1),
                draft,
            };
            app.sync_all(cx);
            return;
        }
        AppModal::ImportRobot { path, .. } => {
            app.st.modal = AppModal::ImportRobot {
                path,
                summary: None,
            };
            app.sync_all(cx);
            return;
        }
        AppModal::ArtifactCleanup { page } => {
            app.st.modal = AppModal::ArtifactCleanup {
                page: page.saturating_sub(1),
            };
            app.sync_all(cx);
            return;
        }
        AppModal::Trash { page } => {
            app.st.modal = AppModal::Trash {
                page: page.saturating_sub(1),
            };
            app.sync_all(cx);
            return;
        }
        _ => {}
    }
    if let AppModal::Wizard { step } = &app.st.modal {
        let s = (*step).max(2) - 1;
        app.st.modal = AppModal::Wizard { step: s };
        app.sync_all(cx);
    }
}

pub fn modal_danger(app: &mut App, cx: &mut Cx) {
    if !app.can_edit() {
        app.sync_all(cx);
        return;
    }
    match &app.st.modal.clone() {
        AppModal::TaskEditor => crate::task_editor::discard(&mut app.st),
        AppModal::RestoreTrash { id } => app.st.modal = AppModal::PurgeTrash { id: id.clone() },
        AppModal::PurgeTrash { id } => {
            let result = app
                .workspace_session
                .as_ref()
                .ok_or_else(|| std::io::Error::other("Workspace is read-only"))
                .and_then(|session| session.purge_artifacts(&mut app.st, id));
            match result {
                Ok(()) => {
                    app.st.modal = AppModal::None;
                    app.st
                        .toast("Trashed files permanently deleted; disk space released".into());
                }
                Err(e) => app.st.toast(e.to_string()),
            }
        }
        AppModal::ArtifactCleanup { .. } => app.commit_artifact_cleanup(),
        AppModal::RemoveRobot => {
            app.preview_robot_removal();
        }
        AppModal::DeleteScene => app.st.del_scene_yes(),
        AppModal::DeleteRun => {
            let keep = app.keep_sel == 0;
            app.st.run_delete_yes(keep);
        }
        _ => {}
    }
    app.sync_all(cx);
}

pub fn modal_ok(app: &mut App, cx: &mut Cx) {
    if !app.can_edit() {
        app.sync_all(cx);
        return;
    }
    let host = app.ui.widget(cx, ids!(modal_host));
    let modal = app.st.modal.clone();
    match modal {
        AppModal::AgentArm => match app.agent_action("agent-arm-confirm", "") {
            Ok(()) => app.st.modal = AppModal::None,
            Err(e) => app.st.toast(e),
        },
        AppModal::AgentCommand => {
            let text = host.widget(cx, ids!(panel.in_wrap0.input0)).text();
            match app.agent_action("agent-set-command", &text) {
                Ok(()) => app.st.modal = AppModal::None,
                Err(e) => app.st.toast(e),
            }
        }
        AppModal::Evaluation => {
            let other = host.widget(cx, ids!(panel.in_wrap0.input0)).text();
            let seconds = host.widget(cx, ids!(panel.in_wrap1.input1)).text();
            let seeds = host.widget(cx, ids!(panel.in_wrap2.input2)).text();
            if let Err(e) = app.start_evaluation(&other, &seconds, &seeds) {
                app.st.toast(e);
            }
        }
        AppModal::EvaluationExport => {
            let path = host.widget(cx, ids!(panel.in_wrap0.input0)).text();
            match app.export_evaluation(&path) {
                Ok(()) => {
                    app.st.modal = AppModal::None;
                    app.st.toast(format!("Report exported to {path}"));
                }
                Err(e) => app.st.toast(e),
            }
        }
        AppModal::SimulationImport => {
            let path = host.widget(cx, ids!(panel.in_wrap0.input0)).text();
            if let Err(e) = app.recover_simulation(&path) {
                app.st.toast(e);
            }
        }
        AppModal::SimulationCommand | AppModal::SimulationPush => {
            let texts = [
                host.widget(cx, ids!(panel.in_wrap0.input0)).text(),
                host.widget(cx, ids!(panel.in_wrap1.input1)).text(),
                host.widget(cx, ids!(panel.in_wrap2.input2)).text(),
            ];
            let result = (|| -> Result<(), String> {
                let values = texts
                    .iter()
                    .map(|s| {
                        s.trim()
                            .parse::<f64>()
                            .map_err(|_| "Enter finite numbers".to_string())
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if values.iter().any(|v| !v.is_finite()) {
                    return Err("Enter finite numbers".into());
                }
                let request = if modal == AppModal::SimulationCommand {
                    if values.iter().any(|v| v.abs() > 10.) {
                        return Err("Velocity commands must be within ±10".into());
                    }
                    serde_json::json!({"op":"command", "value":values})
                } else {
                    if values[0].abs() > 1000.
                        || values[1].abs() > 1000.
                        || !(values[2] > 0. && values[2] <= 5.)
                    {
                        return Err(
                            "Forces must be within ±1000 N; duration in (0, 5] seconds".into()
                        );
                    }
                    serde_json::json!({"op":"push", "force":[values[0],values[1],0.], "seconds":values[2]})
                };
                app.simulation_control(request)
            })();
            match result {
                Ok(()) => app.st.modal = AppModal::None,
                Err(e) => app.st.toast(e),
            }
        }
        AppModal::QueueSettings { page, draft } => {
            let values = [
                host.widget(cx, ids!(panel.in_wrap0.input0)).text(),
                host.widget(cx, ids!(panel.in_wrap1.input1)).text(),
                host.widget(cx, ids!(panel.in_wrap2.input2)).text(),
            ];
            let result = (|| -> Result<(), String> {
                let mut c = draft;
                match page {
                    0 => {
                        c.transport = values[0].trim().into();
                        c.delay_minutes = values[1]
                            .trim()
                            .parse()
                            .map_err(|_| "Delay must be a nonnegative number of minutes")?;
                        c.host = values[2].trim().into();
                        if !["local", "ssh"].contains(&c.transport.as_str()) {
                            return Err("Execution must be local or ssh".into());
                        }
                    }
                    1 => {
                        c.remote_repo = values[0].trim().into();
                        c.remote_root = values[1].trim().into();
                        c.studio_binary = values[2].trim().into();
                    }
                    2 => {
                        c.python = values[0].trim().into();
                        c.port = values[1]
                            .trim()
                            .parse()
                            .map_err(|_| "SSH port must be an integer")?;
                        c.identity = values[2].trim().into();
                    }
                    _ => {
                        c.known_hosts = values[0].trim().into();
                        c.processes = values[1]
                            .trim()
                            .parse()
                            .map_err(|_| "Learner processes must be a positive integer")?;
                        c.validate()?;
                    }
                }
                app.st.modal = if page < 3 {
                    AppModal::QueueSettings {
                        page: page + 1,
                        draft: c,
                    }
                } else {
                    app.st.job_settings = c;
                    AppModal::None
                };
                Ok(())
            })();
            if let Err(error) = result {
                app.st.toast(error);
            }
        }
        AppModal::ReviewEdit => {
            let start = host.widget(cx, ids!(panel.in_wrap0.input0)).text();
            let end = host.widget(cx, ids!(panel.in_wrap1.input1)).text();
            let note = host.widget(cx, ids!(panel.in_wrap2.input2)).text();
            match app.review_edit(&start, &end, note) {
                Ok(()) => {
                    app.st.modal = AppModal::None;
                    app.st.toast("Episode edit saved".into());
                }
                Err(e) => app.st.toast(e),
            }
        }
        AppModal::ReviewExport => {
            let path = host.widget(cx, ids!(panel.in_wrap0.input0)).text();
            let result = app
                .review
                .document
                .as_ref()
                .ok_or("Open a recording".to_string())
                .and_then(|d| crate::review::export(d, std::path::Path::new(path.trim())));
            match result {
                Ok(()) => {
                    app.st.modal = AppModal::None;
                    app.st
                        .toast(format!("Edited recording exported to {}", path.trim()));
                }
                Err(e) => app.st.toast(e),
            }
        }
        AppModal::ImportRobot { path, summary } => {
            if summary.is_some() {
                app.import_robot_path(&path);
            } else {
                let path = host.widget(cx, ids!(panel.in_wrap0.input0)).text();
                let path = path.trim();
                let result = if std::path::Path::new(path)
                    .extension()
                    .is_some_and(|x| x == "urdf")
                {
                    crate::robot_import::inspect(std::path::Path::new(path))
                } else {
                    crate::robot_bundle::inspect(std::path::Path::new(path))
                };
                match result {
                    Ok(summary) => {
                        app.st.modal = AppModal::ImportRobot {
                            path: path.into(),
                            summary: Some(summary),
                        }
                    }
                    Err(e) => app.st.toast(format!("Import review failed: {e}")),
                }
            }
        }
        AppModal::ExportRobot => {
            let destination = host.widget(cx, ids!(panel.in_wrap0.input0)).text();
            let result = app
                .st
                .sel
                .robot
                .as_ref()
                .and_then(|id| app.st.robots.iter().find(|r| &r.id == id))
                .ok_or_else(|| "Select a robot".to_string())
                .and_then(|r| {
                    crate::robot_bundle::export(r, std::path::Path::new(destination.trim()))
                });
            match result {
                Ok(()) => {
                    app.st.modal = AppModal::None;
                    app.st
                        .toast(format!("Robot bundle exported to {}", destination.trim()));
                }
                Err(e) => app.st.toast(format!("Robot export failed: {e}")),
            }
        }
        AppModal::TaskEditor => {
            if let Err(e) = crate::task_editor::accept(&mut app.st) {
                app.st.toast(e);
            }
        }
        AppModal::ExportPolicy { checkpoint } => {
            let destination = host.widget(cx, ids!(panel.in_wrap0.input0)).text();
            if let Err(e) = app.start_policy_export(&checkpoint, &destination) {
                app.st.toast(e);
            }
        }
        AppModal::ArtifactRetention => {
            let count = host
                .widget(cx, ids!(panel.in_wrap0.input0))
                .text()
                .parse::<usize>();
            match count {
                Ok(n) if (1..=1000).contains(&n) => {
                    app.st.retention_keep = n;
                    app.preview_artifacts(None, false);
                }
                _ => app
                    .st
                    .toast("Keep count must be an integer from 1 to 1000".into()),
            }
        }
        AppModal::ArtifactCleanup { page } => {
            if let Some(plan) = &app.artifact_plan {
                if (page + 1) * 5 < plan.description().lines().count() {
                    app.st.modal = AppModal::ArtifactCleanup { page: page + 1 };
                }
            }
        }
        AppModal::Trash { page } => {
            app.st.modal = if (page + 1) * 5 < app.st.trash.len() {
                AppModal::Trash { page: page + 1 }
            } else {
                AppModal::None
            };
        }
        AppModal::RemoveRobot => app.preview_robot_removal(),
        AppModal::RestoreTrash { id } => app.restore_trashed_artifacts(&id),
        AppModal::Tour
        | AppModal::LastSceneBlocked
        | AppModal::RemoveRobotBlocked
        | AppModal::RemoveTargetBlocked { .. } => {
            app.st.modal = AppModal::None;
        }
        AppModal::RenameRobot => {
            let name = host.widget(cx, ids!(panel.in_wrap0.input0)).text();
            if let Err(e) = app.st.rename_robot(&name) {
                app.st.toast(e);
            }
        }
        AppModal::ContinueRun => {
            let name = host.widget(cx, ids!(panel.in_wrap0.input0)).text();
            let count = host
                .widget(cx, ids!(panel.in_wrap1.input1))
                .text()
                .parse::<u32>();
            let checkpoint = host.widget(cx, ids!(panel.in_wrap2.input2)).text();
            match count
                .map_err(|_| "Additional iterations must be a positive integer".to_string())
                .and_then(|n| app.st.continue_run(&checkpoint, &name, n))
            {
                Ok(()) => app.begin_launched_run(cx),
                Err(e) => app.st.toast(e),
            }
        }
        AppModal::RenameSet => {
            let name = host.widget(cx, ids!(panel.in_wrap0.input0)).text();
            app.st.set_rename_yes(&name);
        }
        AppModal::Preflight => {
            let name = host.widget(cx, ids!(panel.in_wrap0.input0)).text();
            let name = if name.is_empty() {
                "crouch-v4".into()
            } else {
                name
            };
            let warm = "Fresh";
            let before = app.st.sel.run.clone();
            app.st.launch(&name, warm);
            // Launch means launch. The row above records what was configured;
            // this starts the engine that fills it.
            if app.st.sel.run != before {
                app.begin_launched_run(cx);
            }
        }
        AppModal::Rerun => {
            let name = host.widget(cx, ids!(panel.in_wrap0.input0)).text();
            let before = app.st.sel.run.clone();
            app.st.rerun_yes(&name);
            if app.st.sel.run != before {
                app.begin_launched_run(cx);
            }
        }
        AppModal::PromoteCk { ck_id } => {
            let name = host.widget(cx, ids!(panel.in_wrap0.input0)).text();
            app.st.ck_promote_yes(&ck_id, &name);
        }
        AppModal::Wizard { step } => {
            if step == 1 && app.st.wiz_file.is_none() {
                let picked = rfd::FileDialog::new()
                    .set_title("Pick a robot description")
                    .add_filter("Robot description (URDF)", &["urdf"])
                    .add_filter("All files", &["*"])
                    .set_directory(std::env::var("MAKEPAD_URDF_DATA").unwrap_or_else(|_| {
                        format!("{}/data", crate::nexus::repo_dir())
                    }))
                    .pick_file();
                if let Some(p) = picked {
                    app.st.wiz_file = Some(p.to_string_lossy().to_string());
                    app.st.modal = AppModal::Wizard { step: 2 };
                }
            } else if step >= 2 {
                app.st.wiz_commit();
                app.st.wiz_file = None;
            } else {
                app.st.modal = AppModal::Wizard { step: step + 1 };
            }
        }
        AppModal::TargetForm { edit } => {
            let name = host.widget(cx, ids!(panel.in_wrap0.input0)).text();
            let ipif = host.widget(cx, ids!(panel.in_wrap1.input1)).text();
            if !app.st.demo {
                let token = host.widget(cx, ids!(panel.in_wrap2.input2)).text();
                if let Err(e) = app.save_agent_target(edit, &name, &ipif, &token) {
                    app.st.toast(e);
                }
                app.sync_all(cx);
                return;
            }
            let capv = host
                .widget(cx, ids!(panel.in_wrap2.input2))
                .text()
                .trim()
                .parse::<u32>()
                .unwrap_or(70);
            let (ip, iface) = match ipif.split_once('·') {
                Some((a, b)) => (a.trim().to_string(), b.trim().to_string()),
                None => (ipif.trim().to_string(), String::new()),
            };
            app.st.target_save(edit, &name, &ip, &iface, capv);
        }
        AppModal::Arm => {
            app.st.dep_arm_yes();
        }
        _ => {}
    }
    app.sync_all(cx);
}

pub fn route_modal(app: &mut App, cx: &mut Cx, actions: &Actions) {
    if app.st.modal == AppModal::None {
        return;
    }
    let host = app.ui.widget(cx, ids!(modal_host));
    if host.is_empty() {
        return;
    }
    if app.st.modal == AppModal::TaskEditor && app.can_edit() {
        let paths = [
            ids!(panel.in_wrap0.input0),
            ids!(panel.in_wrap1.input1),
            ids!(panel.in_wrap2.input2),
        ];
        let mut changed = false;
        for (i, p) in paths.iter().enumerate() {
            if let Some(value) = host.text_input(cx, *p).changed(actions) {
                crate::task_editor::input(&mut app.st, i, value);
                changed = true;
            }
        }
        if changed {
            app.sync_all(cx);
        }
    }
    if host.button(cx, ids!(panel.foot.b_cancel)).clicked(actions) {
        app.st.modal = AppModal::None;
        app.sync_all(cx);
        return;
    }
    if host.button(cx, ids!(panel.foot.b_back)).clicked(actions) {
        modal_back(app, cx);
        return;
    }
    // option rows
    let opaths = [
        ids!(panel.opt0),
        ids!(panel.opt1),
        ids!(panel.opt2),
        ids!(panel.opt3),
        ids!(panel.opt4),
        ids!(panel.opt5),
        ids!(panel.opt6),
        ids!(panel.opt7),
    ];
    for (i, op) in opaths.iter().enumerate() {
        let uid = host.widget(cx, *op).widget_uid();
        if view_clicked(actions, uid) {
            modal_opt(app, cx, i);
            return;
        }
    }
    if host.button(cx, ids!(panel.foot.b_danger)).clicked(actions) {
        modal_danger(app, cx);
        return;
    }
    if host.button(cx, ids!(panel.foot.b_ok)).clicked(actions) {
        modal_ok(app, cx);
    }
}
