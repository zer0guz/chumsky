//! Parser extensions that inspect the input without modifying it.
//!
//! *"Only one man stood and watched the sky, stood with terrible sadness in his eyes
//! and rubber bungs in his ears. He knew exactly what was happening and had known
//! ever since his Sub-Etha Sens-O-Matic had started winking in the dead of night
//! beside his pillar and woken him with a start."*
use super::*;
use crate::input::{Checkpoint, Cursor, InputFor, TokenOf};


/// A type that receives event hooks when certain parsing actions occur.
///
/// If you don't need to receive event hooks, use [`SimpleState`].
pub trait Inspector<I>
where
    I: Input,
{
    /// A type the Inspector can use to revert to a previous state.
    ///
    /// For implementation reasons, this is required to be `Clone`.
    type Checkpoint: Clone;

    /// This function is called when a new token is read from the input stream.
    // impl note: this should be called only when `self.cursor` is updated, not when we only peek at the next token.
    fn on_token(&mut self, token: &TokenOf<'_, I>);
    /// This function is called when a combinator saves the current state of the parse.
    fn on_save<'parse>(&self, cursor: &Cursor<'_, 'parse, I>) -> Self::Checkpoint;
    /// This function is called when a combinator rewinds to an earlier state of the parser.
    ///
    /// You can use [`Checkpoint::inspector`] to get back the [`Checkpoint`][Self::Checkpoint]
    /// you originally created in [`on_save`][Self::on_save].
    fn on_rewind<'parse>(&mut self, marker: &Checkpoint<'_, 'parse, I, Self::Checkpoint>);
}

impl<I> Inspector<I> for ()
where
    I: Input,
{
    type Checkpoint = ();
    #[inline(always)]
    fn on_token(&mut self, _: &<I as InputFor<'_>>::Token) {}
    #[inline(always)]
    fn on_save<'parse>(&self, _: &Cursor<'_, 'parse, I>) -> Self::Checkpoint {}
    #[inline(always)]
    fn on_rewind<'parse>(&mut self, _: &Checkpoint<'_, 'parse, I, Self>) {}
}

/// A state type that should be accessible directly from `parser.state()` and has no special behavior.
///
/// This wrapper implements the [`Inspector`] trait for you so you don't have to.
#[derive(Copy, Clone, Default, Debug)]
pub struct SimpleState<T>(pub T);
impl<T, I> Inspector<I> for SimpleState<T>
where
    I:  Input,
{
    type Checkpoint = ();
    #[inline(always)]
    fn on_token(&mut self, _: &TokenOf<'_,I>) {}
    #[inline(always)]
    fn on_save<'parse>(&self, _: &Cursor<'_, 'parse, I>) -> Self::Checkpoint {}
    #[inline(always)]
    fn on_rewind<'parse>(&mut self, _: &Checkpoint<'_, 'parse, I, Self::Checkpoint>) {}
}

impl<T> Deref for SimpleState<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for SimpleState<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> From<T> for SimpleState<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

/// A state type that clones and rolls back its contents during a rewind.
///
/// This might be useful if you want to use the parser state to, say, count the parsed occurrences of a particular
/// construct.
///
/// Ideally, you should try to have the [`Clone`] implementation be fairly cheap.
#[derive(Copy, Clone, Default, Debug)]
pub struct RollbackState<T>(pub T);
impl<T, I> Inspector<I> for RollbackState<T>
where
    I:  Input,
    T: Clone,
{
    type Checkpoint = T;
    #[inline(always)]
    fn on_token(&mut self, _: &TokenOf<'_,I>) {}
    #[inline(always)]
    fn on_save<'parse>(&self, _: &Cursor<'_, 'parse, I>) -> Self::Checkpoint {
        self.0.clone()
    }
    #[inline(always)]
    fn on_rewind<'parse>(&mut self, cp: &Checkpoint<'_, 'parse, I, Self::Checkpoint>) {
        self.0 = cp.inspector.clone();
    }
}

impl<T> Deref for RollbackState<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for RollbackState<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> From<T> for RollbackState<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

/// A state type that encapsulates a vector, truncating the vector to its original size during a rewind.
///
/// This might be useful for representing, say, an arena-style allocator.
#[derive(Clone, Default, Debug)]
pub struct TruncateState<T>(pub Vec<T>);
impl<T, I> Inspector<I> for TruncateState<T>
where
    I: Input,
    T: Clone,
{
    type Checkpoint = usize;
    #[inline(always)]
    fn on_token(&mut self, _: &TokenOf<'_,I>) {}
    #[inline(always)]
    fn on_save<'parse>(&self, _: &Cursor<'_, 'parse, I>) -> Self::Checkpoint {
        self.0.len()
    }
    #[inline(always)]
    fn on_rewind<'parse>(&mut self, cp: &Checkpoint<'_, 'parse, I, Self::Checkpoint>) {
        self.0.truncate(cp.inspector);
    }
}

impl<T> Deref for TruncateState<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for TruncateState<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> From<Vec<T>> for TruncateState<T> {
    fn from(value: Vec<T>) -> Self {
        Self(value)
    }
}
