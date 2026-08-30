//! A question with no bound on it — a confirmation prompt, a browser round
//! trip — outlives every precondition checked before it was asked. Five
//! commands cross one, and each used to re-take its preconditions by hand,
//! the rule living in site comments (ADR an-invariant-gets-a-door). This is
//! the door: a command hands over the question and the re-asks the wait made
//! stale, and only a run of both, in that order, yields a [`Fresh`]. Which
//! preconditions go stale, and in what order to re-take them, stays each
//! command's own.

use crate::error::Result;

/// Evidence that a wait's re-establish has run. The field is private, so the
/// only way to hold one is out of [`across`] — an irreversible step that takes
/// `&Fresh` cannot be reached on answers from before the wait.
#[derive(Debug)]
pub struct Fresh(());

impl Fresh {
    /// A witness with no wait behind it, for a unit test of a step downstream
    /// of one.
    #[cfg(test)]
    pub fn for_a_test() -> Self {
        Fresh(())
    }
}

/// What a question somebody may turn down came back with. Its own enum rather
/// than `Option`, because `None` reads as "no answer" and a decline is one.
pub enum Asked<T> {
    Declined,
    Answered(T),
}

/// The question, then the re-establish, then the witness. `holding` is
/// whatever both halves must reach — the registry hold, usually. A question
/// that fails is never followed by the re-asks, because nothing downstream of
/// it will run.
pub fn across<S, T, R>(
    holding: &mut S,
    question: impl FnOnce(&mut S) -> Result<T>,
    again: impl FnOnce(&mut S) -> Result<R>,
) -> Result<(T, R, Fresh)> {
    let answered = question(holding)?;
    let re_established = again(holding)?;
    Ok((answered, re_established, Fresh(())))
}

/// [`across`], for a question that takes no for an answer. A decline runs no
/// re-establish and yields no witness: a command that will do nothing has no
/// precondition left to make current.
pub fn across_unless_declined<S, T, R>(
    holding: &mut S,
    question: impl FnOnce(&mut S) -> Result<Asked<T>>,
    again: impl FnOnce(&mut S) -> Result<R>,
) -> Result<Option<(T, R, Fresh)>> {
    match question(holding)? {
        Asked::Declined => Ok(None),
        Asked::Answered(answered) => {
            let re_established = again(holding)?;
            Ok(Some((answered, re_established, Fresh(()))))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::PerchError;

    /// The one re-establish every test here hands over, so the two tests
    /// asserting it never ran name the same step the first test proves runs.
    fn re_establishes(ran: &mut Vec<&'static str>) -> Result<&'static str> {
        ran.push("re-established");
        Ok("current")
    }

    #[test]
    fn the_re_establish_runs_after_the_question_and_before_the_witness() {
        let mut ran: Vec<&str> = Vec::new();

        let (answered, re_established, _witness) = across(
            &mut ran,
            |ran| {
                ran.push("asked");
                Ok("an answer")
            },
            re_establishes,
        )
        .expect("both halves succeed");

        assert_eq!(ran, ["asked", "re-established"]);
        assert_eq!(answered, "an answer");
        assert_eq!(re_established, "current");
    }

    #[test]
    fn a_re_establish_that_fails_yields_no_witness() {
        let refused = across(
            &mut (),
            |_| Ok("answered"),
            |_| -> Result<()> { Err(PerchError::Other("gone stale".to_string())) },
        )
        .expect_err("the re-establish refused");

        assert!(refused.to_string().contains("gone stale"), "{refused}");
    }

    #[test]
    fn a_decline_runs_nothing_and_yields_no_witness() {
        let mut ran: Vec<&str> = Vec::new();

        let crossed = across_unless_declined(
            &mut ran,
            |ran| {
                ran.push("asked");
                Ok(Asked::<()>::Declined)
            },
            re_establishes,
        )
        .expect("a decline is not a failure");

        assert!(crossed.is_none());
        assert_eq!(ran, ["asked"]);
    }

    #[test]
    fn a_question_that_fails_never_reaches_the_re_establish() {
        let mut ran: Vec<&str> = Vec::new();

        across(
            &mut ran,
            |_| -> Result<()> { Err(PerchError::Other("no terminal".to_string())) },
            re_establishes,
        )
        .expect_err("the question failed");

        assert!(ran.is_empty(), "{ran:?}");
    }
}
