#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Command {
    Noop,
    Start,
    End,
}

pub(super) fn parse_command(buf: &[u8]) -> Result<Command, ()> {
    match buf {
        b"" => Ok(Command::Noop),
        b"start\n" => Ok(Command::Start),
        b"end\n" => Ok(Command::End),
        _ => Err(()),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SessionState {
    Idle,
    Active,
    Analyzing,
}

impl SessionState {
    pub(super) const fn new() -> Self {
        Self::Idle
    }

    pub(super) fn try_start(&mut self) -> Result<(), ()> {
        if *self != Self::Idle {
            return Err(());
        }
        *self = Self::Active;
        Ok(())
    }

    pub(super) fn try_begin_analysis(&mut self) -> Result<(), ()> {
        if *self != Self::Active {
            return Err(());
        }
        *self = Self::Analyzing;
        Ok(())
    }

    pub(super) fn finish_analysis(&mut self) {
        debug_assert_eq!(*self, Self::Analyzing);
        *self = Self::Idle;
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use std::{
        sync::{Arc, Mutex},
        thread,
        vec::Vec,
    };

    use super::*;

    #[test]
    fn parses_complete_commands_and_empty_noop() {
        assert_eq!(parse_command(b""), Ok(Command::Noop));
        assert_eq!(parse_command(b"start\n"), Ok(Command::Start));
        assert_eq!(parse_command(b"end\n"), Ok(Command::End));
    }

    #[test]
    fn rejects_unknown_and_fragmented_commands() {
        for command in [b"start".as_slice(), b"end", b"garbage\n"] {
            assert_eq!(parse_command(command), Err(()));
        }
    }

    #[test]
    fn completes_session() {
        let mut state = SessionState::new();
        assert_eq!(state.try_start(), Ok(()));
        assert_eq!(state.try_begin_analysis(), Ok(()));
        state.finish_analysis();
        assert_eq!(state, SessionState::Idle);
    }

    #[test]
    fn rejects_end_before_start_without_changing_state() {
        let mut state = SessionState::new();
        assert_eq!(state.try_begin_analysis(), Err(()));
        assert_eq!(state, SessionState::Idle);
    }

    #[test]
    fn rejects_repeated_start_without_changing_state() {
        let mut state = SessionState::new();
        assert_eq!(state.try_start(), Ok(()));
        assert_eq!(state.try_start(), Err(()));
        assert_eq!(state, SessionState::Active);
    }

    #[test]
    fn analyzing_rejects_start_and_end() {
        let mut state = SessionState::Active;
        assert_eq!(state.try_begin_analysis(), Ok(()));
        assert_eq!(state.try_start(), Err(()));
        assert_eq!(state.try_begin_analysis(), Err(()));
        assert_eq!(state, SessionState::Analyzing);
    }

    #[test]
    fn failed_transition_allows_later_valid_session() {
        let mut state = SessionState::new();
        assert_eq!(state.try_begin_analysis(), Err(()));
        assert_eq!(state.try_start(), Ok(()));
        assert_eq!(state.try_begin_analysis(), Ok(()));
        state.finish_analysis();
        assert_eq!(state.try_start(), Ok(()));
    }

    #[test]
    fn concurrent_start_has_one_winner() {
        let state = Arc::new(Mutex::new(SessionState::new()));
        let workers: Vec<_> = (0..8)
            .map(|_| {
                let state = state.clone();
                thread::spawn(move || state.lock().unwrap().try_start())
            })
            .collect();
        let successes = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .filter(Result::is_ok)
            .count();

        assert_eq!(successes, 1);
        assert_eq!(*state.lock().unwrap(), SessionState::Active);
    }
}
