//! The headless surfaces: what the engine can be asked to do without a window.
//!
//! These live in the engine crate rather than in a UI because they are pure
//! std and depend on nothing but the engine itself — which means both consoles
//! get them for free by calling [`maybe_headless`] before they start drawing,
//! and a script gets them without starting a GUI at all.
//!
//! `--json` is a modifier rather than a mode: every surface here honours it and
//! emits one object per line. That stream is the contract other products read;
//! the fixed-width text beside it exists for humans and is free to change.

// `zealot` is always compiled — it is pure std, and the feature decides whether
// a console prefers that backend, not whether the client exists.
use crate::{ckpt, crosssim, env, json, probe, rng, scene, sweep, trainer, zealot};

/// Did the caller ask for machine-readable output?
///
/// A modifier rather than a mode: every headless surface honours it, so a
/// consumer learns one flag instead of one flag per capability.
fn wants_json() -> bool {
    std::env::args().any(|a| a == "--json")
}

/// One interval as a human reads it. Fixed-width, and free to change — the
/// contract other products consume is the JSON beside it, not this.
fn sample_line(s: &trainer::Sample) -> String {
    format!(
        "{:>9} steps  reward {:>6.3}  falls {:>5.1}%  ep_len {:>5.0}  {:>6.0}k steps/s",
        s.step,
        s.reward,
        s.fall_rate * 100.0,
        s.episode_len,
        s.steps_per_sec / 1000.0
    )
}

/// The same interval as a machine reads it.
fn sample_json(s: &trainer::Sample) -> String {
    // `terms` is positional in `Sample`; pairing it with `TERM_NAMES` here means
    // a consumer never has to know the order, and a future reweighting cannot
    // silently relabel a column.
    let mut terms = json::Obj::new();
    for (i, name) in env::TERM_NAMES.iter().enumerate() {
        if let Some(v) = s.terms.get(i) {
            terms = terms.f32(name, *v, 5);
        }
    }
    json::Obj::new()
        .str("event", "sample")
        .int("step", s.step)
        .f32("reward", s.reward, 5)
        .f32("fall_rate", s.fall_rate, 5)
        .f32("episode_len", s.episode_len, 2)
        .num("steps_per_sec", s.steps_per_sec, 1)
        .obj("terms", terms)
        .done()
}

/// `--headless N` trains without a window and prints progress. The fastest way
/// to answer "is it learning" without judging it through a UI.
///
/// With `--json` the same run emits one object per line — a `start`, one
/// `sample` per interval, then an `end`. That stream is the contract the
/// console reads; scraping the human line above was the alternative and it
/// couples a consumer to a format that can change without a compile error.
fn headless(total: u64) -> ! {
    let envs = 256;
    // --no-random trains at nominal physics, which is the control case for
    // showing that the sweep measures anything at all.
    let randomize = !std::env::args().any(|a| a == "--no-random");
    let json = wants_json();

    // With --features zealot and a built stack, the same loop reports zealot's
    // GPU run. `total` stays a budget in env-steps for both backends: zealot
    // counts iterations, and at 256 envs it emits 24 steps per env per
    // iteration, so the budget converts rather than changing meaning.
    #[cfg(feature = "zealot")]
    let (h, backend, note) = match zealot::spawn(
        envs,
        (total / (envs as u64 * 24)).max(1),
        "dorobot_nexus.safetensors",
    ) {
        Some(h) => (h, "zealot", zealot::binary_path().display().to_string()),
        None => (
            trainer::spawn_with(envs, total, 1, randomize),
            "cpu",
            format!(
                "no zealot binary at {}; run scripts/setup-zealot.sh",
                zealot::binary_path().display()
            ),
        ),
    };
    #[cfg(not(feature = "zealot"))]
    let (h, backend, note) = (
        trainer::spawn_with(envs, total, 1, randomize),
        "cpu",
        String::from("built without the zealot feature"),
    );

    // Which backend ran is part of the result, not decoration: the same command
    // means different physics on a machine with the GPU stack built.
    if json {
        println!(
            "{}",
            json::Obj::new()
                .str("event", "start")
                .str("backend", backend)
                .str("backend_note", &note)
                .usize("envs", envs)
                .int("total_steps", total)
                .bool("randomize", randomize)
                .done()
        );
    } else {
        println!("backend: {backend} ({note})");
    }

    let mut shown = 0usize;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(250));
        // Take every interval published since the last poll, not just the
        // newest. The trainer can publish more than once per 250 ms, and a
        // stream that silently drops rows is worse than one that is late.
        let (fresh, done) = {
            let g = h.shared.lock().unwrap();
            let from = shown.min(g.samples.len());
            (g.samples[from..].to_vec(), !g.running)
        };
        shown += fresh.len();
        for s in &fresh {
            println!("{}", if json { sample_json(s) } else { sample_line(s) });
        }
        if done {
            break;
        }
    }

    if json {
        println!(
            "{}",
            json::Obj::new()
                .str("event", "end")
                .usize("samples", shown)
                .done()
        );
    }
    std::process::exit(0);
}

/// `--sweep` runs the robustness sweep on the newest checkpoint and prints it.
/// A surface you can only see in a GUI is a surface you cannot diff between
/// two policies, which is the comparison that makes it worth computing.
fn headless_sweep() -> ! {
    let json = wants_json();
    let Some(surface) = sweep::spawn(trainer::RUN_ID) else {
        fail(json, "no checkpoint to sweep");
    };
    loop {
        std::thread::sleep(std::time::Duration::from_millis(150));
        let g = surface.lock().unwrap();
        if !g.running {
            if json {
                let mass: Vec<f32> = (0..sweep::ROWS).map(sweep::axis_mass).collect();
                let force: Vec<f32> = (0..sweep::COLS).map(sweep::axis_force).collect();
                let cells: Vec<Vec<Option<f64>>> = (0..sweep::ROWS)
                    .map(|r| (0..sweep::COLS).map(|c| g.cell(r, c).map(f64::from)).collect())
                    .collect();
                println!(
                    "{}",
                    json::Obj::new()
                        .str("event", "sweep")
                        .usize("rows", sweep::ROWS)
                        .usize("cols", sweep::COLS)
                        .nums("axis_mass", &mass, 4)
                        .nums("axis_force", &force, 4)
                        .grid("cells", &cells, 5)
                        .f32("pass_fraction", g.pass_fraction(), 5)
                        .done()
                );
            } else {
                println!("mass\\force   {}", (0..sweep::COLS)
                    .map(|c| format!("{:>5.2}", sweep::axis_force(c)))
                    .collect::<Vec<_>>().join(" "));
                for r in 0..sweep::ROWS {
                    let cells: Vec<String> = (0..sweep::COLS)
                        .map(|c| match g.cell(r, c) {
                            Some(v) => format!("{:>5.2}", v),
                            None => "    —".into(),
                        })
                        .collect();
                    println!("{:>9.2}   {}", sweep::axis_mass(r), cells.join(" "));
                }
                println!("\n{:.0}% of cells pass", g.pass_fraction() * 100.0);
            }
            break;
        }
    }
    std::process::exit(0);
}

/// `--track-check` asks the one question the flat sweep raised: does the policy
/// respond to its velocity command at all?
fn track_check() -> ! {
    use crate::env::{VecEnv, N_ACT, N_OBS};
    use crate::rl::{Config, Ppo};
    let json = wants_json();
    let Some((path, m)) = ckpt::list(trainer::RUN_ID).into_iter().next() else {
        fail(json, "no checkpoint");
    };
    let (w, _) = ckpt::read(&path).unwrap();
    let mut rng = rng::Rng::new(1);
    let hidden = if m.hidden == 0 { 64 } else { m.hidden };
    let mut ppo = Ppo::new(N_OBS, N_ACT, hidden, Config::default(), &mut rng);
    assert!(ppo.load_weights(&w));

    if !json {
        println!("  cmd    mean dx   tracked");
    }
    for k in -3..=3 {
        let cmd = k as f32 * 0.25;
        let mut env = VecEnv::new(1, 5);
        env.restart(0, cmd);
        let (mut obs, mut act) = (vec![0.0; N_OBS], vec![0.0; N_ACT]);
        let (mut sum_dx, mut n) = (0.0_f32, 0usize);
        for _ in 0..400 {
            env.observe(0, &mut obs);
            ppo.act_mean(&obs, &mut act);
            let o = env.step(&[act.clone()]);
            // obs[1] is cart velocity.
            env.observe(0, &mut obs);
            sum_dx += obs[1];
            n += 1;
            if o.done[0] { break; }
        }
        let dx = sum_dx / n.max(1) as f32;
        let tracked = (-3.0 * (dx - cmd).abs()).exp();
        if json {
            println!(
                "{}",
                json::Obj::new()
                    .str("event", "track_check")
                    .f32("cmd", cmd, 5)
                    .f32("mean_dx", dx, 5)
                    .f32("tracked", tracked, 5)
                    .done()
            );
        } else {
            println!("{cmd:>6.2} {dx:>10.3} {tracked:>9.2}");
        }
    }
    std::process::exit(0);
}

/// Report a headless failure in whichever format the caller asked for.
///
/// In JSON mode this goes to stdout, not stderr: a consumer reading the stream
/// should learn why a run produced nothing from the stream itself rather than
/// having to correlate two pipes.
fn fail(json: bool, message: &str) -> ! {
    if json {
        println!(
            "{}",
            json::Obj::new()
                .str("event", "error")
                .str("message", message)
                .done()
        );
    } else {
        eprintln!("{message}");
    }
    std::process::exit(1);
}

/// The optional run id following a flag.
///
/// `--probe --json` must not read `--json` as a run name, so anything starting
/// with `--` means "not an argument" and the default run stands.
fn run_arg(args: &[String], i: usize) -> String {
    match args.get(i + 1) {
        Some(a) if !a.starts_with("--") => a.clone(),
        _ => trainer::RUN_ID.to_string(),
    }
}

/// The zealot checkpoint to drive. One definition, shared by the GUI and the
/// headless surfaces, so they cannot disagree about the default.
///
/// Live only in zealot builds, like the module it names — same reason
/// `mod zealot` carries the attribute rather than being `#[cfg]`-gated.
#[allow(dead_code)]
pub fn zealot_ckpt_path() -> String {
    std::env::var("DOROBOT_ZEALOT_CKPT").unwrap_or_else(|_| "dorobot_nexus.safetensors".to_string())
}

/// `--capabilities` prints what this build can actually do.
///
/// `zealot` is a compile-time feature, so one binary name means different
/// capabilities on different machines, and testing that a file exists at a path
/// answers one bit — the wrong one. Anything deciding whether to offer the GPU
/// path, `--curriculum`, or the JSON contract should ask here instead of
/// guessing from a filename.
fn capabilities() -> ! {
    let json = wants_json();
    let features: &[&str] = if cfg!(feature = "zealot") { &["zealot"] } else { &[] };

    // Compiled in is not the same as usable. zealot is driven as a subprocess,
    // so the feature says only that the client exists; the binary decides
    // whether the backend can actually run.
    let zealot_bin = zealot::binary_path();
    let zealot_ready = cfg!(feature = "zealot") && zealot_bin.is_file();
    let mut backends = vec!["cpu"];
    if zealot_ready {
        backends.push("zealot");
    }

    // Every flag `maybe_headless` dispatches, so a caller can test for a
    // capability by name rather than infer it from a version number.
    let mut entry_points = vec![
        "--capabilities",
        "--crosssim",
        "--headless",
        "--json",
        "--no-random",
        "--probe",
        "--sweep",
        "--track-check",
    ];
    if cfg!(feature = "zealot") {
        entry_points.push("--curriculum");
        entry_points.sort_unstable();
    }

    if json {
        println!(
            "{}",
            json::Obj::new()
                .str("event", "capabilities")
                .str("name", env!("CARGO_PKG_NAME"))
                .str("version", env!("CARGO_PKG_VERSION"))
                .strs("features", features)
                .strs("backends", &backends)
                .strs("entry_points", &entry_points)
                .bool("zealot_binary", zealot_bin.is_file())
                .str("zealot_binary_path", &zealot_bin.display().to_string())
                .str("run_id", trainer::RUN_ID)
                .done()
        );
    } else {
        println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
        println!(
            "features:     {}",
            if features.is_empty() { "(none)".to_string() } else { features.join(" ") }
        );
        println!("backends:     {}", backends.join(" "));
        println!("entry points: {}", entry_points.join(" "));
        println!("run id:       {}", trainer::RUN_ID);
    }
    std::process::exit(0);
}

fn score_obj(s: &crosssim::Score) -> json::Obj {
    json::Obj::new()
        .f32("survival", s.survival, 5)
        .f32("tracking", s.tracking, 5)
        .f32("reward", s.reward, 5)
        .f32("fall_rate", s.fall_rate, 5)
}

/// `--crosssim [run]` runs sim-to-sim validation on a run's newest checkpoint
/// and prints the comparison.
///
/// Validate has computed this since the screen existed, but only from a GUI
/// event handler — so no other process could reach it at any price. This is
/// that route, and it is why the flag exists at all.
fn headless_crosssim(run: &str) -> ! {
    let json = wants_json();

    // Mirror the GUI's preference: with a built zealot stack the meaningful
    // comparison is decimation against decimation, not Euler against RK4.
    #[cfg(feature = "zealot")]
    let started = crosssim::zealot_cross::spawn(&zealot_ckpt_path())
        .or_else(|| crosssim::spawn(run));
    #[cfg(not(feature = "zealot"))]
    let started = crosssim::spawn(run);

    let Some(report) = started else {
        fail(json, &format!("no checkpoint to compare for run {run}"));
    };

    loop {
        std::thread::sleep(std::time::Duration::from_millis(150));
        if !report.lock().unwrap().running {
            break;
        }
    }

    let r = report.lock().unwrap();
    // `done` separates a comparison from a run that merely stopped: on a
    // weight/manifest mismatch the report carries the reason in `label` and no
    // scores at all, and reporting those zeros as a result would be a lie.
    if json {
        println!(
            "{}",
            json::Obj::new()
                .str("event", "crosssim")
                .str("label", &r.label)
                .bool("done", r.done)
                .f32("worst_gap", r.worst_gap(), 5)
                .obj("a", score_obj(&r.a))
                .obj("b", score_obj(&r.b))
                .done()
        );
    } else if r.done {
        println!("{}", r.label);
        println!("{:<14} {:>8} {:>8} {:>8}", "", "A", "B", "delta");
        for (name, a, b, d) in r.rows() {
            println!("{name:<14} {a:>8} {b:>8} {d:>8}");
        }
        println!("\nworst gap {:.1}%", r.worst_gap() * 100.0);
    } else {
        eprintln!("{}", r.label);
    }
    std::process::exit(if r.done { 0 } else { 1 });
}

/// One probe state, as the driving process reads it.
fn emit_probe(p: &probe::Probe, json: bool, event: &str) {
    let f = p.frame();
    if json {
        println!(
            "{}",
            json::Obj::new()
                .str("event", event)
                .str("label", &p.label)
                .usize("cursor", p.cursor)
                .usize("frames", p.frames.len())
                .num("progress", p.progress(), 5)
                .opt_usize("last_push", p.last_push)
                .f32("cart_x", f.cart_x, 5)
                .f32("angle", f.angle, 5)
                .f32("action", f.action, 5)
                .f32("reward", f.reward, 5)
                .bool("fell", f.fell)
                .done()
        );
    } else {
        println!(
            "{:>4}/{:<4} x {:>7.3}  angle {:>7.3}  action {:>6.3}  reward {:>6.3}{}",
            p.cursor,
            p.frames.len(),
            f.cart_x,
            f.angle,
            f.action,
            f.reward,
            if f.fell { "  FELL" } else { "" }
        );
    }
}

/// A bad command reports and the session continues — the driving process
/// should not lose its probe to a typo.
fn emit_probe_error(json: bool, message: &str) {
    if json {
        println!(
            "{}",
            json::Obj::new()
                .str("event", "error")
                .str("message", message)
                .done()
        );
    } else {
        eprintln!("{message}");
    }
}

/// `--probe [run]` drives a checkpoint one command at a time, reading commands
/// on stdin and reporting the state each one produced.
///
/// The probe is what the design calls the differentiator: the simulation runs
/// in this process, so stepping it is a function call and pushing it is another
/// one. Until now that was reachable only by hand from the engine's own Inspect
/// screen. A console that wants to show a push has to be able to ask for one.
///
/// Commands, one per line:
///   `step [n]`   advance the cursor by n frames; negative steps back
///   `seek <t>`   jump to fraction t of the recording, 0..1
///   `push <dv>`  shove the cart by dv and re-simulate from the cursor
///   `restart`    fresh episode from the same checkpoint
///   `state`      re-report without changing anything
///   `quit`       exit, as does EOF
fn headless_probe(run: &str) -> ! {
    use std::io::BufRead;

    let json = wants_json();
    let Some(mut probe) = probe::Probe::load_latest(run) else {
        fail(json, &format!("no usable checkpoint for run {run}"));
    };
    // The probe records an episode on load, so there is a state to report
    // before the first command arrives.
    emit_probe(&probe, json, "ready");

    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        // EOF or a closed pipe is a normal end: that is how a driving process
        // is expected to stop this loop.
        let Ok(line) = line else { break };
        let mut it = line.split_whitespace();
        let Some(cmd) = it.next() else { continue };
        let arg = it.next();
        match cmd {
            "quit" | "exit" => break,
            "state" => emit_probe(&probe, json, "state"),
            "restart" => {
                probe.restart();
                emit_probe(&probe, json, "state");
            }
            "step" => {
                let n = arg.and_then(|a| a.parse::<i32>().ok()).unwrap_or(1);
                probe.step_by(n);
                emit_probe(&probe, json, "state");
            }
            "seek" => match arg.and_then(|a| a.parse::<f64>().ok()) {
                Some(t) => {
                    probe.seek_fraction(t);
                    emit_probe(&probe, json, "state");
                }
                None => emit_probe_error(json, "seek needs a fraction, e.g. `seek 0.5`"),
            },
            "push" => match arg.and_then(|a| a.parse::<f32>().ok()) {
                Some(dv) => {
                    probe.push(dv);
                    emit_probe(&probe, json, "state");
                }
                None => emit_probe_error(json, "push needs a velocity, e.g. `push 1.5`"),
            },
            other => emit_probe_error(json, &format!("unknown command: {other}")),
        }
    }
    std::process::exit(0);
}

/// `--curriculum [iters_each] [rounds]` trains across the saved scene set,
/// resuming the same checkpoint as the world changes under it.
#[cfg(feature = "zealot")]
fn headless_curriculum(iters_each: u64, rounds: usize) -> ! {
    let scenes = scene::list();
    println!("curriculum over {} scene(s), {iters_each} iters each x {rounds} round(s):", scenes.len());
    for sc in &scenes {
        println!("  {:<28} {}", sc.name, sc.summary());
    }
    let set: Vec<(String, Vec<(&'static str, String)>)> =
        scenes.iter().map(|s| (s.name.clone(), s.knobs())).collect();
    let Some(h) = zealot::spawn_curriculum(set, 256, iters_each, rounds, "curriculum.safetensors")
    else {
        eprintln!("no zealot binary — run scripts/setup-zealot.sh");
        std::process::exit(1);
    };
    let mut shown = 0usize;
    loop {
        std::thread::sleep(std::time::Duration::from_millis(400));
        let (n, done, line) = {
            let g = h.shared.lock().unwrap();
            let line = g.samples.last().map(|s| {
                format!(
                    "{:>9} steps  reward {:>7.4}  falls {:>5.1}%",
                    s.step, s.reward, s.fall_rate * 100.0
                )
            });
            (g.samples.len(), !g.running, line)
        };
        if n > shown {
            if let Some(l) = line {
                println!("{l}");
            }
            shown = n;
        }
        if done {
            break;
        }
    }
    println!("curriculum finished");
    std::process::exit(0);
}

pub fn maybe_headless() {
    let args: Vec<String> = std::env::args().collect();
    // First, because it is the flag a caller uses to discover the others and it
    // must work on a build where everything else is unavailable.
    if args.iter().any(|a| a == "--capabilities") {
        capabilities();
    }
    if args.iter().any(|a| a == "--track-check") {
        track_check();
    }
    if let Some(i) = args.iter().position(|a| a == "--probe") {
        headless_probe(&run_arg(&args, i));
    }
    if let Some(i) = args.iter().position(|a| a == "--crosssim") {
        headless_crosssim(&run_arg(&args, i));
    }
    #[cfg(feature = "zealot")]
    if let Some(i) = args.iter().position(|a| a == "--curriculum") {
        let iters = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(20);
        let rounds = args.get(i + 2).and_then(|v| v.parse().ok()).unwrap_or(1);
        headless_curriculum(iters, rounds);
    }
    if args.iter().any(|a| a == "--sweep") {
        headless_sweep();
    }
    if let Some(i) = args.iter().position(|a| a == "--headless") {
        let n: u64 = args.get(i + 1).and_then(|v| v.parse().ok()).unwrap_or(2_000_000);
        headless(n);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sample_names_its_reward_terms() {
        // `Sample::terms` is positional. Pairing it with TERM_NAMES at the edge
        // means a consumer never has to know the order, and a reweighting
        // cannot silently relabel a column.
        let s = trainer::Sample {
            step: 8192,
            reward: 0.5,
            terms: vec![0.1, 0.2, 0.3, 0.4],
            fall_rate: 0.25,
            steps_per_sec: 60000.0,
            episode_len: 24.0,
            leans: vec![],
        };
        let out = sample_json(&s);
        assert!(out.contains(r#""event":"sample""#));
        assert!(out.contains(r#""step":8192"#));
        assert!(out.contains(r#""track_lin_vel":0.10000"#), "{out}");
        assert!(out.contains(r#""torque":0.40000"#), "{out}");
    }

    #[test]
    fn a_sample_with_fewer_terms_than_names_omits_rather_than_invents() {
        // A checkpoint from an older reward shape must not have its missing
        // terms reported as zero — an absent term and a zero term differ.
        let s = trainer::Sample { terms: vec![0.1], ..Default::default() };
        let out = sample_json(&s);
        assert!(out.contains("track_lin_vel"));
        assert!(!out.contains("torque"), "{out}");
    }

    #[test]
    fn a_non_finite_metric_does_not_break_the_stream() {
        // A diverged run produces NaN. Emitting the literal `NaN` would make the
        // line unparseable, taking the whole stream down with it.
        let s = trainer::Sample { reward: f32::NAN, ..Default::default() };
        assert!(sample_json(&s).contains(r#""reward":null"#));
    }

    #[test]
    fn a_flag_is_never_read_as_a_run_id() {
        // `--probe --json` must probe the default run, not a run called "--json".
        let args: Vec<String> = ["dorobot-nexus", "--probe", "--json"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(run_arg(&args, 1), trainer::RUN_ID);
    }

    #[test]
    fn an_explicit_run_id_is_taken() {
        let args: Vec<String> = ["dorobot-nexus", "--probe", "other-run", "--json"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(run_arg(&args, 1), "other-run");
    }

    #[test]
    fn a_trailing_flag_falls_back_to_the_default_run() {
        let args: Vec<String> = ["dorobot-nexus", "--crosssim"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(run_arg(&args, 1), trainer::RUN_ID);
    }
}
