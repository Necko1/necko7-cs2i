#[derive(Debug, PartialEq)]
pub enum CloseAction {
    Hide,
    Shutdown,
    Quitting,
}
pub fn close_action(minimize: bool, quitting: bool) -> CloseAction {
    if quitting {
        CloseAction::Quitting
    } else if minimize {
        CloseAction::Hide
    } else {
        CloseAction::Shutdown
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn close_policy_and_exit_are_independent_of_autostart() {
        assert_eq!(close_action(true, false), CloseAction::Hide);
        assert_eq!(close_action(false, false), CloseAction::Shutdown);
        assert_eq!(close_action(true, true), CloseAction::Quitting);
        assert_eq!(close_action(false, true), CloseAction::Quitting);
    }
}
