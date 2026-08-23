//! A string holding a secret, which is a buffer with two rules rather than one.
//!
//! `Zeroizing<String>` wipes what it holds when it is dropped and says nothing
//! about the buffers it held before that. A `String` frees the block it grew out
//! of, so every doubling of one carrying a refresh token hands a prefix of the
//! token back to the allocator untouched — the fragment is longer each time, and
//! the last one is nearly the whole token. Five reviews in a row found one of
//! those, in five different buffers, each fixed by reserving at the finished
//! width at that call site. The growth is here instead, once, and it wipes
//! (ADR an-invariant-gets-a-door).

use zeroize::{Zeroize, Zeroizing};

/// A string that wipes what it holds, including whatever it grew out of.
///
/// The width a caller states makes growth rare rather than safe: an under-count
/// costs one copy and wipes what it left, so the arithmetic is an optimization
/// and never a correctness argument.
#[derive(Default, Clone)]
pub struct Secret {
    held: Zeroizing<String>,
}

impl Secret {
    /// A buffer with room for `width` bytes before it has to grow.
    pub fn with_room_for(width: usize) -> Secret {
        Secret {
            held: Zeroizing::new(String::with_capacity(width)),
        }
    }

    /// A secret that arrives whole. `String::from(&str)` reserves exactly, so
    /// nothing here grows.
    pub fn copied(value: &str) -> Secret {
        Secret {
            held: Zeroizing::new(value.to_string()),
        }
    }

    /// A `String` that already holds one, taken over rather than copied — the
    /// copy would be a second buffer to wipe.
    pub fn taken_over(held: String) -> Secret {
        Secret {
            held: Zeroizing::new(held),
        }
    }

    pub fn push_str(&mut self, text: &str) {
        self.room_for(text.len());
        self.held.push_str(text);
    }

    pub fn push(&mut self, c: char) {
        self.room_for(c.len_utf8());
        self.held.push(c);
    }

    pub fn as_str(&self) -> &str {
        &self.held
    }

    /// The room reserved, which is what a test asserts the property on: a
    /// buffer whose capacity is what it holds is one that never moved.
    pub fn capacity(&self) -> usize {
        self.held.capacity()
    }

    /// The whole of what this type is for: `String` grows by allocating a wider
    /// block, copying into it and freeing the old one, and the free is what it
    /// does not wipe. Done here, the old block is wiped first.
    fn room_for(&mut self, more: usize) {
        let needed = self.held.len() + more;
        if needed <= self.held.capacity() {
            return;
        }
        // Doubling as `String` does, so a caller that under-counted by a byte
        // does not pay a copy per push after it.
        let mut wider = String::with_capacity(needed.max(self.held.capacity() * 2));
        wider.push_str(&self.held);
        let mut left = std::mem::replace(&mut *self.held, wider);
        left.zeroize();
    }
}

/// So a `Secret` can be handed to anything taking a `&str` — a request body, a
/// header value, a child's stdin — without a copy that would be a second buffer
/// nothing wipes.
impl std::ops::Deref for Secret {
    type Target = str;

    fn deref(&self) -> &str {
        &self.held
    }
}

impl std::fmt::Debug for Secret {
    /// The size, never the bytes, for the reason [`crate::host::Execution`]'s is
    /// redacted: a `Secret` in a `Debug` is one in a panic message and in a log.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "Secret(<{} bytes>)", self.held.len())
    }
}

/// For the callers that build one through `write!` rather than by pushing.
impl std::fmt::Write for Secret {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.push_str(text);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The claim the type exists for, asserted on the allocator rather than on
    /// the buffer: what a grown `String` leaves behind is freed heap, which a
    /// test cannot read. So the buffer's own identity stands in — a `Secret`
    /// that grew is one whose old block was wiped before it was let go.
    #[test]
    fn a_secret_that_outgrows_its_room_wipes_what_it_grew_out_of() {
        let mut secret = Secret::with_room_for(4);
        let was_at = secret.as_str().as_ptr();

        secret.push_str("sk-ant-ort01-a-refresh-token");

        assert_ne!(
            secret.as_str().as_ptr(),
            was_at,
            "it moved, which is the case this is about"
        );
        assert_eq!(secret.as_str(), "sk-ant-ort01-a-refresh-token");
    }

    /// The ordinary path: a width the caller counted right is a buffer that
    /// never moves, so there is nothing to wipe until it is dropped.
    #[test]
    fn a_secret_with_room_for_what_it_holds_never_moves() {
        let mut secret = Secret::with_room_for(32);
        let was_at = secret.as_str().as_ptr();

        secret.push_str("sk-ant-oat01-an-access-token");
        secret.push('\n');

        assert_eq!(secret.as_str().as_ptr(), was_at);
    }

    /// A `Secret` reaches a panic message the moment one is `unwrap`ped in a
    /// struct holding it, and a derived `Debug` would print the token.
    #[test]
    fn nothing_about_a_secret_prints_it() {
        let secret = Secret::copied("sk-ant-ort01-a-refresh-token");

        let said = format!("{secret:?}");

        assert!(!said.contains("sk-ant"), "{said}");
        assert!(said.contains("28 bytes"), "{said}");
    }
}
