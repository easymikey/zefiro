use std::fmt::Debug;

use kernel::update::Machine;

pub(crate) type Cell<S> = Result<(S, <S as Machine>::Effect), <S as Machine>::Error>;

pub(crate) fn cell<S>(start: S, message: S::Message, expected: Cell<S>)
where
    S: Machine + Clone + Debug + PartialEq,
    S::Effect: Debug + PartialEq,
    S::Error: Debug + PartialEq,
{
    let before = start.clone();
    let mut state = start;
    match (state.update(message), expected) {
        (Ok(effect), Ok(wanted)) => assert_eq!((state, effect), wanted),
        (Err(reason), Err(wanted)) => {
            assert_eq!(state, before);
            assert_eq!(reason, wanted);
        }
        (Ok(effect), Err(wanted)) => {
            panic!("expected a refusal ({wanted:?}), got {state:?} with {effect:?}");
        }
        (Err(reason), Ok(_)) => panic!("{reason:?}"),
    }
}
