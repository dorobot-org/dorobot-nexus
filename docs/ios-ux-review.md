# iOS-style Studio UX review

Target: **91/100 for each Studio**, using the fixed [rubric](../tools/ux/ios-rubric.json). The target is not yet accepted. Final acceptance requires the complete capture matrix, interaction journeys, restart checks, and a scorecard tied to the tested source.

## What changed in OctoSense

The reference is OctoSense main at `388e2f3882c1d7b3776fee2dc6b4d875abecc557` (November 13, 2026). Its latest desktop work is **macOS-specific**; the phone and desktop trees remain separate. We use its testing and iteration method while choosing iPad-style navigation for the Studios.

| Period | Improvement | Lesson applied here |
| --- | --- | --- |
| September 2026 | Shared phone/desktop shell; [iOS search behavior](https://github.com/OctoSense-org/octosense/pull/168) with keyboard handling and per-keystroke results | Test navigation, typing and selection together |
| September–October | [Hidden app lifecycle](https://github.com/OctoSense-org/octosense/commit/b741da3e), [durable action cards](https://github.com/OctoSense-org/octosense/blob/388e2f3882c1d7b3776fee2dc6b4d875abecc557/docs/adr/0007-composable-mail-action-cards.md), and [bounded background jobs](https://github.com/OctoSense-org/octosense/blob/388e2f3882c1d7b3776fee2dc6b4d875abecc557/docs/adr/0008-quiet-android-mail-jobs.md) | Preserve the task when navigating and keep UI state separate from running work |
| October | [Gesture/frame pacing work](https://github.com/OctoSense-org/octosense/pull/463) and [opening built apps without Cargo](https://github.com/OctoSense-org/octosense/pull/552) | Measure actual runtime behavior, including cold and warm launch limits |
| November | [Fixed-rubric desktop review](https://github.com/OctoSense-org/octosense/pull/560), original pixels, repeatable journeys and Mac-mini execution | Repair observed defects, then capture and rerun the affected journeys |

PR #560 reports a 94.4 average across 38 desktop surfaces: 14 reached 95 and another 13 reached 94. Its journey batches passed 860/860 and 1043/1043; OctosCode passed 134/136. These are the project's published results, not independently reproduced here. Earlier scores covered fewer surfaces, so they do not form a controlled numerical before/after comparison. They also do not establish iPhone acceptance for these Studios.

Apple's [button guidance](https://developer.apple.com/design/human-interface-guidelines/buttons) informs 44-point primary targets, and its [sidebar guidance](https://developer.apple.com/design/human-interface-guidelines/sidebars) informs persistent navigation and readable split views.

## Changes across both Studios

- Persistent workspace navigation, a current-page heading and an explicit appearance control.
- Nano replay gives space to the robot. Cameras appear when media exists; episodes, details and signals can be shown or hidden. Compact windows swap the episode list for details.
- Recording, hardware and evaluation explain missing setup instead of showing idle instruments as if they were live.
- Appearance, current page, dataset, selected episode and playhead are saved locally. Restoring a session never starts playback or recording. Explicit launch arguments take precedence.
- Shared fonts, semantic surfaces and rounded controls match Nexus. Keyboard shortcuts use Command/Control + 1–5 in Studio and 1–8 in Nexus.
- Embedded robot pose shortcuts are disabled so Select All and typing cannot animate the robot or alter its pose.

## Reproduce the review

Use isolated workspaces and the generated Nano preview. Keep all recordings, preferences, reports and captures outside Git. The `DOROBOT_STUDIO_STATE_DIR` override isolates Studio preferences; `DOROBOT_NEXUS_DIR` and `NEXUS_RUNS_DIR` isolate Nexus state.

Set `MAKEPAD_HIDE_WINDOWS=1` and `DOROBOT_UX_REPORT` to an absolute output file when launching a test instance. The report is opt-in and read-only: it accepts no commands. The drivers send real pointer and keyboard input using Makepad's remote interface.

```sh
python3 tools/ux/capture_matrix.py --app nexus --pid "$test_pid" \
  --remote "$makepad_remote" --report "$audit_dir/state.json" --out "$audit_dir/matrix"
python3 tools/ux/exercise.py --app nexus --pid "$test_pid" \
  --remote "$makepad_remote" --report "$audit_dir/state.json" --out "$audit_dir/journeys"
```

Use `--app nexus` for Nexus. Launch each process with its own report file. Never target a person's working window. A copied Relax executable also requires its matching standard-library sources through `MAKEPAD_RUN_STD`; the normal launcher bootstrap retains these in its pinned source checkout.

The fixed matrix is every screen at 1440×900 and 1024×768, in light and dark. Keep original captures and their hashes; do not retouch screenshots. Scores are model-authored reviews, not independent human grades. The final scorecard must list deductions and scope limits.

Physical robot capture, FFmpeg video, real training throughput and iPhone deployment require separate acceptance. Relax is beta: Studio's dormant numeric dependencies currently emit background compilation diagnostics; successful Nano replay does not establish that every dependency path works.
