//! The in-app keybindings editor.

use super::*;

impl AppState {
    pub fn open_keybindings(&mut self) {
        self.show_keybindings = true;
        self.keybindings_capture = None;
        self.keybindings_conflict = None;
        self.keybindings_reset_confirm = false;
        self.keybindings_status = None;
        let last = crate::keybindings::action_defs().len().saturating_sub(1);
        self.keybindings_selected = self.keybindings_selected.min(last);
    }

    pub fn close_keybindings(&mut self) {
        self.show_keybindings = false;
        self.keybindings_capture = None;
        self.keybindings_conflict = None;
        self.keybindings_reset_confirm = false;
    }

    pub fn keybindings_selected_action(&self) -> crate::keybindings::KeyAction {
        crate::keybindings::action_defs()[self.keybindings_selected].action
    }

    pub fn keybindings_move(&mut self, delta: isize) {
        let len = crate::keybindings::action_defs().len() as isize;
        let next = (self.keybindings_selected as isize).saturating_add(delta).clamp(0, (len - 1).max(0));
        self.keybindings_selected = next as usize;
        self.keybindings_status = None;
    }

    pub fn keybindings_select(&mut self, index: usize) {
        let last = crate::keybindings::action_defs().len().saturating_sub(1);
        self.keybindings_selected = index.min(last);
        self.keybindings_status = None;
    }

    pub fn keybindings_start_capture(&mut self, action: crate::keybindings::KeyAction) {
        self.keybindings_capture = Some(action);
        self.keybindings_conflict = None;
        self.keybindings_reset_confirm = false;
        self.keybindings_status = None;
    }

    pub fn keybindings_apply_capture(&mut self, chord: crate::keybindings::KeyChord) {
        let Some(action) = self.keybindings_capture.take() else {
            return;
        };
        let label = crate::keybindings::def_for(action).label;
        if let Some(other) = self.keybindings.conflict(action, chord) {
            self.keybindings_conflict = Some((action, chord, other));
            return;
        }
        self.keybindings.set_only(action, chord);
        self.keybindings.save();
        self.keybindings_status = Some(format!("bound {} → {label}", chord.display()));
    }

    pub fn keybindings_resolve_conflict(&mut self, accept: bool) {
        let Some((action, chord, _)) = self.keybindings_conflict.take() else {
            return;
        };
        if accept {
            self.keybindings.unbind(chord);
            self.keybindings.set_only(action, chord);
            self.keybindings.save();
            let label = crate::keybindings::def_for(action).label;
            self.keybindings_status = Some(format!("reassigned {} → {label}", chord.display()));
        }
    }

    pub fn keybindings_clear_selected(&mut self) {
        let action = self.keybindings_selected_action();
        self.keybindings.clear(action);
        self.keybindings.save();
        let label = crate::keybindings::def_for(action).label;
        self.keybindings_status = Some(format!("cleared {label}"));
    }

    pub fn keybindings_reset_selected(&mut self) {
        let action = self.keybindings_selected_action();
        self.keybindings.reset(action);
        self.keybindings.save();
        let label = crate::keybindings::def_for(action).label;
        self.keybindings_status = Some(format!("reset {label}"));
    }

    pub fn keybindings_reset_all(&mut self) {
        self.keybindings.reset_all();
        self.keybindings.save();
        self.keybindings_reset_confirm = false;
        self.keybindings_status = Some("all shortcuts reset to defaults".to_string());
    }
}
