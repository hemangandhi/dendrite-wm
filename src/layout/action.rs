use smithay::backend::input::{InputBackend, InputEvent, KeyState, KeyboardKeyEvent};
use smithay::input::keyboard::{Keysym, KeysymHandle, ModifiersState};

#[derive(Clone, Copy, Debug)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug)]
pub enum Action {
    MoveFocus(Direction),
    MoveNode(Direction),
    MakeInnerTree,
    CloseWindow,
    Spawn,
}

impl Action {
    pub fn from_key_event<I: InputBackend>(
        event: &InputEvent<I>,
        mods: &ModifiersState,
        keysym: KeysymHandle,
    ) -> Option<Self> {
        let InputEvent::Keyboard { event } = event else {
            return None;
        };
        if !(event.state() == KeyState::Pressed) || !mods.alt {
            return None;
        }
        match keysym.raw_latin_sym_or_raw_current_sym() {
            Some(Keysym::Return) => Some(Action::Spawn),
            Some(Keysym::q) => Some(Action::CloseWindow),
            Some(Keysym::h) => {
                if mods.ctrl {
                    Some(Action::MoveNode(Direction::Left))
                } else {
                    Some(Action::MoveFocus(Direction::Left))
                }
            }
            Some(Keysym::j) => {
                if mods.ctrl {
                    Some(Action::MoveNode(Direction::Down))
                } else {
                    Some(Action::MoveFocus(Direction::Down))
                }
            }
            Some(Keysym::k) => {
                if mods.ctrl {
                    Some(Action::MoveNode(Direction::Up))
                } else {
                    Some(Action::MoveFocus(Direction::Up))
                }
            }
            Some(Keysym::l) => {
                if mods.ctrl {
                    Some(Action::MoveNode(Direction::Right))
                } else {
                    Some(Action::MoveFocus(Direction::Right))
                }
            }
            Some(Keysym::semicolon) => Some(Action::MakeInnerTree),
            _ => None,
        }
    }
}
