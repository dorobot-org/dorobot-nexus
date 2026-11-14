//! Native pickers run on the HAL thread; answers retain their original task.
use crate::{state::AppModal, App};
use makepad_widgets::*;
use std::path::PathBuf;

pub(crate) enum Pending {
    Checkpoint(PathBuf),
    Profile(Option<String>),
    Import(AppModal),
    Wizard,
}

impl Pending {
    fn id(&self) -> LiveId {
        match self {
            Self::Checkpoint(_) => live_id!(checkpoint_export),
            Self::Profile(_) => live_id!(training_profile),
            Self::Import(_) => live_id!(robot_import),
            Self::Wizard => live_id!(wizard_robot),
        }
    }
}

impl App {
    pub(crate) fn route_file_dialogs(&mut self, cx: &mut Cx, actions: &Actions) {
        for action in actions {
            let Some(dialog) = action.downcast_ref::<FileDialogAction>() else {
                continue;
            };
            if !self
                .pending_dialog
                .as_ref()
                .is_some_and(|p| p.id() == dialog.id())
            {
                continue;
            }
            let selected = match dialog {
                FileDialogAction::FileSelected { paths, .. } => paths.first(),
                FileDialogAction::SaveFileSelected { path, .. } => Some(path),
                FileDialogAction::FileCancelled { .. }
                | FileDialogAction::SaveFileCancelled { .. } => None,
                _ => continue,
            };
            let pending = self.pending_dialog.take().unwrap();
            // Cancellation leaves drafts, selections and existing files alone.
            if let Some(path) = selected {
                match pending {
                    Pending::Checkpoint(source) => match export_checkpoint(&source, path) {
                        Ok(()) => self
                            .st
                            .toast(format!("Checkpoint exported to {}", path.display())),
                        Err(error) => self.st.toast(error),
                    },
                    Pending::Profile(robot) if self.st.sel.robot == robot => {
                        if let Some(r) = robot
                            .as_ref()
                            .and_then(|id| self.st.robots.iter_mut().find(|r| &r.id == id))
                        {
                            r.use_task = false;
                        }
                        std::env::set_var("NEXUS_TRAINING_PROFILE", path);
                        self.st
                            .toast(format!("Training backend: {}", path.display()));
                    }
                    Pending::Import(original) if self.st.modal == original => {
                        self.st.modal = AppModal::ImportRobot {
                            path: path.to_string_lossy().into_owned(),
                            summary: None,
                        };
                    }
                    Pending::Wizard if self.st.modal == (AppModal::Wizard { step: 1 }) => {
                        self.st.wiz_file = Some(path.to_string_lossy().into_owned());
                        self.st.modal = AppModal::Wizard { step: 2 };
                    }
                    _ => self
                        .st
                        .toast("Selection ignored because the active task changed.".into()),
                }
            }
            self.sync_all(cx);
        }
    }
}

fn export_checkpoint(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> Result<(), String> {
    // Selecting the source itself must not truncate the checkpoint.
    if source
        .canonicalize()
        .ok()
        .zip(destination.canonicalize().ok())
        .is_some_and(|(a, b)| a == b)
    {
        return Err("Choose a different destination from the original checkpoint.".into());
    }
    std::fs::copy(source, destination).map_err(|e| format!("Export failed: {e}"))?;
    if source.with_extension("txt").is_file() {
        std::fs::copy(
            source.with_extension("txt"),
            destination.with_extension("txt"),
        )
        .map_err(|e| format!("Weights copied, but manifest export failed: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_preserves_original_and_copies_manifest() {
        let dir = std::env::temp_dir().join(format!("nexus-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let source = dir.join("original.pt");
        let dest = dir.join("export.pt");
        std::fs::write(&source, b"weights").unwrap();
        std::fs::write(source.with_extension("txt"), b"manifest").unwrap();
        assert!(export_checkpoint(&source, &source).is_err());
        assert_eq!(std::fs::read(&source).unwrap(), b"weights");
        export_checkpoint(&source, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"weights");
        assert_eq!(
            std::fs::read(dest.with_extension("txt")).unwrap(),
            b"manifest"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
