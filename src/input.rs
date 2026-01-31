//! Token input streams and tools converting to and from them..
//!
//! *“What’s up?” “I don’t know,” said Marvin, “I’ve never been there.”*
//!
//! [`Input`] is the primary trait used to feed input data into a chumsky parser. You can create them in a number of
//! ways: from strings, slices, arrays, etc.

use inspector::Inspector;

pub use crate::stream::{BoxedExactSizeStream, BoxedStream, IterInput, Stream};
use crate::{
    error::ErrorHkt,
    extra::{CtxOf, ErrOfEx},
    util::Ref,
};

use super::*;
use alloc::string::ToString;
#[cfg(feature = "std")]
use std::io::{BufReader, Read, Seek};

pub trait InputFor<'src, T, ImplicitBounds: Sealed = Bounds<&'src Self>> {
    /// The token type returned by [`Input::next_maybe`], allowing abstracting over by-value and by-reference inputs.
    ///
    /// This type should be defines as either
    ///
    /// ```ignore
    /// type MaybeToken = I::Token;
    /// ```
    ///
    /// or
    ///
    /// ```ignore
    /// type MaybeToken = &'src I::Token;
    /// ```
    ///
    /// Some inputs (like `&str`) can *only* generate their tokens by-value (a `&str` does not exactly contain any
    /// addressable `char`s due to the nature of [UTF-8 encoding](https://en.wikipedia.org/wiki/UTF-8#Description)).
    ///
    /// Other inputs (like `&[T]`) can *only* generate their tokens by-reference (`T` might not implement `Clone` or
    /// might be expensive to clone, but we still want to be able to parse slices containing `T`s).
    ///
    /// Combinators like [`just`] need to be able to work with both categories of input, and `MaybeToken` is the glue
    /// that allows that to happen. It does this by providing a way of expressing that a token is 'maybe borrowed', in
    /// a similar way to [`std::borrow::Cow`] or [`std::convert::AsRef`], via the [`IntoMaybe`] trait.
    ///
    /// It is good practice for inputs that generate their tokens by-value to implement [`ValueInput`], and inputs that
    /// generate their tokens by-reference to implement [`BorrowInput`]. Additionally, it is perfectly fine for inputs
    /// to implement *both* of these traits at once.
    type MaybeToken: IntoMaybe<'src, T>;

    /// The type used to keep track of the current location in the stream.
    ///
    /// Chumsky regularly copies cursors around to perform input rewinding (for the sake of
    /// [backtracking](https://en.wikipedia.org/wiki/Backtracking), and other things). Therefore, cursors should be
    /// cheap to clone and should have trivial [`Drop`] logic where possible. If your input type needs to perform more
    /// substantial bookkeeping, you should use [`Input::Cache`] to store heavy-weight input state and keep cursors as
    /// lightweight as possible, such as simple indices into the cache data.
    type Cursor: Clone;

    /// A type that contains cached or constant data pertaining to an input.
    ///
    /// Most input types (such as `&[T]` and `&str`) don't need a cache because they're just pulling tokens straight
    /// out of memory. If your input type falls into a similar category, `()` can be used. However, some inputs -
    /// like [`Stream`] - need to do extra bookkeeping to support backtracking, and a cache facilitates this behaviour.
    type Cache;
}

pub type CacheOf<'src, I> = <I as InputFor<'src, <I as Input>::Token>>::Cache;
pub type CursorOf<'src, I> = <I as InputFor<'src, <I as Input>::Token>>::Cursor;
pub type MaybeTokenOf<'src, I> = <I as InputFor<'src, <I as Input>::Token>>::MaybeToken;

/// A trait for types that represents a stream of input tokens. Unlike [`Iterator`], this type
/// supports backtracking and a few other features required by the crate.
///
/// `Input` abstracts over streams which yield tokens by value or reference, and which may or may not have
/// slices of tokens taken from them for use in parser output. There are multiple traits that inherit from
/// `Input` and are implemented by types to indicate support for these specific abilities. Various combinators
/// on the [`Parser`] trait may require that the input type implement one or more of these more specific traits.
///
/// Some common input types, and which traits they implement are:
/// - `&str`: [`SliceInput`], [`StrInput`], [`ValueInput`], [`ExactSizeInput`]
/// - `&[T]`: [`SliceInput`], [`ValueInput`], [`BorrowInput`], [`ExactSizeInput`]
/// - `Stream<I>`: [`ValueInput`], [`ExactSizeInput`] if `I: ExactSizeIterator`
/// - `IterInput<I>`: [`ValueInput`], [`ExactSizeInput`] if `I: ExactSizeIterator`
pub trait Input: for<'src> InputFor<'src, Self::Token> {
    /// The type of a span on this input.
    ///
    /// Spans allow the output of a parser to be tagged with the location that they appear in the input. This is
    /// important for tools that want to produce ergonomic error messages for users to read.
    ///
    /// For an example of how spans can be used by a parser, see [`Parser::map_with`].
    ///
    /// If you want to change the input's span before using it for parsing, see [`Input::map_span`], [`Input::map`],
    /// and [`Input::with_context`].
    type Span: Span;

    /// The type of singular tokens generated by the input.
    ///
    /// This should be the 'by-value' type of tokens emitted. For example, `<&[T] as Input>::Token = T` and
    /// `<&str as Input>::Token = char`. Your token does not need to be capable of directly emitting tokens of this
    /// type (see [`Input::MaybeToken`]).
    type Token;
    /// Create an initial cursor and cache at the start of the input.
    fn begin<'src>(self) -> (CursorOf<'src, Self>, CacheOf<'src, Self>);

    /// Return the 'location' associated with the given cursor.
    ///
    /// Unfortunately, 'location' is not yet a well-defined concept. It is used internally by chumsky to compare the
    /// location of errors and input tokens, but there is no particular requirement that cursor locations be contiguous
    /// or start from zero - chumsky only *compares* locations with one-another.
    ///
    /// We would recommend that you make the cursor location monotonic. This means that each successive cursor position
    /// should have an increasing location. Most of the time using an index or increasing memory address is perfectly
    /// sufficient. Some inputs might also choose to use the start offset of a span, although we'd recommend against
    /// this if your input's spans are non-continuous.
    ///
    /// There are no particular safety requirements placed on this function. You can probably cause chumsky to generate
    /// absurd error messages or take bizarre parse paths through the input [if you abuse it](https://xkcd.com/221/),
    /// however.
    fn cursor_location<'src>(cursor: &CursorOf<'_, Self>) -> usize;

    /// Pull the next token, if any, from the input.
    ///
    /// For alternatives with stronger guarantees, see [`ValueInput::next`] and `BorrowInput::next_ref`.
    ///
    /// The vast majority of input types can implement either [`ValueInput`] or [`BorrowInput`], so the implementation
    /// of this function usually just calls their corresponding `next` function.
    ///
    /// # Safety
    ///
    /// `cursor` must be generated by `Input::begin`, and must not be shared between multiple inputs.
    unsafe fn next_maybe<'src>(
        cache: &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<MaybeTokenOf<'src, Self>>;

    /// Create a span going from the start cursor to the end cursor (exclusive).
    ///
    /// # Safety
    ///
    /// As with [`Input::next_maybe`], the cursors passed to this function must be generated by [`Input::begin`] and
    /// must not be shared between multiple inputs.
    unsafe fn span<'src>(
        cache: &mut CacheOf<'src, Self>,
        range: Range<&CursorOf<'src, Self>>,
    ) -> Self::Span;

    // /// Split an input that produces tokens of type `(T, S)` into one that produces tokens of type `T` and spans of
    // /// type `S`.
    // ///
    // /// This is commonly required for lexers that generate token-span tuples. For example, `logos`'
    // /// [`SpannedIter`](https://docs.rs/logos/0.12.0/logos/struct.Lexer.html#method.spanned) lexer generates such
    // /// pairs.
    // ///
    // /// Also required is an 'End Of Input' (EoI) span. This span is arbitrary, but is used by the input to produce
    // /// sensible spans that extend to the end of the input or are zero-width. Most implementations simply use some
    // /// equivalent of `len..len` (i.e: a span where both the start and end cursors are set to the end of the input).
    // /// However, what you choose for this span is up to you: but consider that the context, start, and end of the span
    // /// will be recombined to create new spans as required by the parser.
    // ///
    // /// Although `Spanned` does implement [`BorrowInput`], please be aware that, as you might anticipate, the slices
    // /// will be those of the original input (usually `&[(T, S)]`) and not `&[T]` so as to avoid the need to copy
    // /// around sections of the input.
    // fn spanned<T, S>(self, eoi: S) -> SpannedInput<T, S, Self>
    // where
    //     Self: Input<'src, Token = (T, S)> + Sized,
    //     T: 'src,
    //     S: Span + Clone + 'src,
    // {
    //     SpannedInput {
    //         input: self,
    //         eoi,
    //         phantom: PhantomData,
    //     }
    // }

    /// Add extra context within spans generated by this input.
    ///
    /// This is useful if you wish to include extra context that applies to all spans emitted during a parse, such as
    /// an identifier that corresponds to the file the spans originated from.
    ///
    /// Returns spans containing your provided context as the Span::Context
    fn with_context<S: Span>(self, context: S::Context) -> WithContext<S, Self>
    where
        Self: Sized,
    {
        WithContext {
            input: self,
            context,
            phantom: EmptyPhantom::new(),
        }
    }

    // /// Map tokens of the input into separate token and span types, using the provided function.
    // ///
    // /// This is useful when you want your parser to consume tokens that have their span carried with them (lexers will
    // /// usually emit such tokens).
    // ///
    // /// Also required is an 'End of Input' (EoI) span. This span is arbitrary, but is used by the input to produce
    // /// sensible spans that extend to the end of the input or are zero-width. Most implementations simply use some
    // /// equivalent of `len..len` (i.e: a span where both the start and end cursors are set to the end of the input).
    // /// However, what you choose for this span is up to you: but consider that the context, start, and end of the span
    // /// will be recombined to create new spans as required by the parser.
    // ///
    // /// Although `MappedInput` does implement [`SliceInput`], please be aware that, as you might anticipate, the slices
    // /// will be those of the original input and not `&[T]`, to avoid the need to copy around sections of the input.
    fn map<T, S: Span, F>(self, eoi: S, f: F) -> MappedInput<Self, T, S, F>
    where
        Self: Sized,
        F: for<'any> InputMapper<'any, MaybeTokenOf<'any, Self>, T, S>,
    {
        MappedInput {
            input: self,
            eoi,
            mapper: f,
            _tok: EmptyPhantom::new(),
        }
    }

    // /// Take an input (such as a slice) with token type `(T, S)` and turns it into one that produces tokens of type `T` and spans of type `S`.
    // ///
    // /// This is useful when you can't make your parser generic over an input type and need to name the input type, such as with [`Parser::nested_in`].
    // ///
    // /// This is largely an ergonomic convenience over calling [`input.map(eoi, |(t, s)| (t, s))`](`Input::map`).
    // ///
    // /// # Examples
    // /// ```
    // /// use chumsky::{input::{Input as _, MappedInput}, prelude::*};
    // ///
    // /// enum TokenTree<'src> { Num(i64), Sum(&'src [(Self, SimpleSpan)]) }
    // ///
    // /// type Input = MappedInput<'src, TokenTree<'src>, SimpleSpan, &'src [(TokenTree<'src>, SimpleSpan)]>;
    // ///
    // /// // This parser will parse a recursive input composed of `TokenTree`s.
    // /// fn eval<'src>() -> impl Parser<'src, Input, i64> {
    // ///     recursive(|tree| {
    // ///         let sum = tree.repeated().fold(0, |sum, x| sum + x)
    // ///             .nested_in(select_ref! { TokenTree::Sum(xs) = e => xs.split_token_span(e.span()) });
    // ///
    // ///         select_ref! { TokenTree::Num(x) => *x }.or(sum)
    // ///     })
    // /// }
    // ///
    // /// let example = [
    // ///     (TokenTree::Sum(&[
    // ///         (TokenTree::Num(1), SimpleSpan::from(0..1)), // 1
    // ///         (TokenTree::Sum(&[
    // ///             (TokenTree::Num(2), SimpleSpan::from(2..3)), // 2
    // ///             (TokenTree::Num(3), SimpleSpan::from(4..5)), // 3
    // ///         ]), SimpleSpan::from(2..5))
    // ///     ]), SimpleSpan::from(0..5)),
    // /// ];
    // ///
    // /// assert_eq!(eval().parse(example[..].split_token_span((0..4).into())).into_result(), Ok(6));
    // /// ```
    // fn split_token_span<T, S>(self, eoi: S) -> MappedInput<Self, T, S>
    // where
    //     Self: Input<Token = (T, S)> + Sized,
    //     for<'src> MaybeTokenOf<'src,Self>: IntoMaybe<'src, (T, S)>,
    //     S: Span + Clone,
    // {
    //     todo!()
    //     //self.map(eoi, util::split_ref::<T, S>)
    // }

    // /// Take an input (such as a slice) with token type `T` where  `T` is a spanned type, and split it into its inner value and token.
    // ///
    // /// See the documentation for [`Input::split_token_span`], which is nearly identical in concept.
    // fn split_spanned<T, S>(self, eoi: S) -> MappedInput<Self, T, S>
    // where
    //     Self: Input<Token = S::Spanned> + Sized,
    //     for<'src> MaybeTokenOf<'src,Self>: IntoMaybe<'src, S::Spanned>,
    //     S: WrappingSpan<T>,
    // {
    //     todo!()
    //     // self.map(eoi, |spanned, lt| {
    //     //     (S::inner_of(spanned), S::span_of(spanned))
    //     // })
    // }

    /// Map the spans output for this input to a different output span.
    ///
    /// This is useful if you wish to include extra context that applies to all spans emitted during a parse, such as
    /// an identifier that corresponds to the file the spans originated from.
    fn map_span<S: Span, F>(self, map_fn: F) -> MappedSpan<S, Self, F>
    where
        Self: Input + Sized,
        F: for<'src> SpanMapper<'src, Self::Span, S>,
    {
        MappedSpan {
            input: self,
            map_fn,
            phantom: PhantomData,
        }
    }
}

/// Implemented by inputs that have a known size.
///
/// In this case, 'size' means 'number of generated tokens/spans'. You can think of this trait as filling the same
/// niche as [`ExactSizeIterator`].
pub trait ExactSizeInput: Input {
    /// Get a span from a start cursor to the end of the input.
    ///
    /// # Safety
    ///
    /// As with functions on [`Input`], the cursors provided must be generated by this input.
    unsafe fn span_from<'src>(
        cache: &mut CacheOf<'src, Self>,
        range: RangeFrom<&CursorOf<'src, Self>>,
    ) -> Self::Span;
}

pub trait SliceInputFor<'src> {
    type Slice: Clone;
}

pub type SliceOf<'src, I> = <I as SliceInputFor<'src>>::Slice;

/// Implemented by inputs from which slices can be extracted.
///
/// Note that there's nothing to suggest that the slice *needs* to contain instances of the token type: it's quite
/// valid for an input to produce symbolic tokens, but slices corresponding to the original source code. However, it is
/// conventional to do so.
pub trait SliceInput: ExactSizeInput + for<'src> SliceInputFor<'src> {
    /// Get the full slice of the input
    ///
    /// # Safety
    ///
    /// As with functions on [`Input`], the cursors provided must be generated by this input.
    fn full_slice<'src>(cache: &mut CacheOf<'src, Self>) -> SliceOf<'src, Self>;

    /// Get a slice from a start and end cursor
    ///
    /// # Safety
    ///
    /// As with functions on [`Input`], the cursors provided must be generated by this input.
    unsafe fn slice<'src>(
        cache: &mut CacheOf<'src, Self>,
        range: Range<&CursorOf<'src, Self>>,
    ) -> SliceOf<'src, Self>;

    /// Get a slice from a start cursor to the end of the input
    ///
    /// # Safety
    ///
    /// As with functions on [`Input`], the cursors provided must be generated by this input.
    unsafe fn slice_from<'src>(
        cache: &mut CacheOf<'src, Self>,
        from: RangeFrom<&CursorOf<'src, Self>>,
    ) -> SliceOf<'src, Self>;
}

/// A trait for types that represent string-like streams of input tokens.
///
/// Currently, this trait is only implemented by [`&[u8]` and `&str` (and other inputs that behave like them).
///
/// This trait is sealed because future versions of chumsky might place extra requirements upon inputs that implement
/// it.
pub trait StrInput: Sealed + ValueInput<Cursor = usize> + SliceInput
where
    Self::Token: Char,
{
    #[doc(hidden)]
    fn stringify<'src>(slice: SliceOf<'src, Self>) -> String;
}

/// Implemented by inputs that can have produce tokens by value.
pub trait ValueInput: Input {
    /// Get the next cursor from the provided one, and the next token if it exists
    ///
    /// # Safety
    ///
    /// As with functions on [`Input`], the cursors provided must be generated by this input.
    unsafe fn next<'src>(
        cache: &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<Self::Token>;
}

/// Implemented by inputs that can have tokens borrowed from them.
///
/// The borrowed tokens have the same lifetime as the original input.
pub trait BorrowInput: Input {
    /// Borrowed version of [`ValueInput::next`] with the same safety requirements.
    ///
    /// # Safety
    ///
    /// As with functions on [`Input`], the cursors provided must be generated by this input.
    unsafe fn next_ref<'src>(
        cache: &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<&'src Self::Token>;
}

impl InputFor<'_, char> for &'_ str {
    type Cursor = usize;

    type MaybeToken = char;

    type Cache = Self;
}

impl Input for &str {
    type Span = SimpleSpan<usize>;

    type Token = char;
    #[inline]
    fn begin<'src>(self) -> (CursorOf<'src, Self>, CacheOf<'src, Self>) {
        (0, self)
    }

    #[inline]
    fn cursor_location<'src>(cursor: &CursorOf<'src, Self>) -> usize {
        *cursor
    }

    #[inline(always)]
    unsafe fn next_maybe<'src>(
        this: &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<MaybeTokenOf<'src, Self>> {
        if *cursor < this.len() {
            // SAFETY: `cursor < self.len()` above guarantees cursor is in-bounds
            //         We only ever return cursors that are at a character boundary
            let c = unsafe {
                this.get_unchecked(*cursor..)
                    .chars()
                    .next()
                    .unwrap_unchecked()
            };
            *cursor += c.len_utf8();
            Some(c)
        } else {
            None
        }
    }

    #[inline(always)]
    unsafe fn span<'src>(
        _this: &mut CacheOf<'src, Self>,
        range: Range<&CursorOf<'src, Self>>,
    ) -> Self::Span {
        (*range.start..*range.end).into()
    }
}

impl ExactSizeInput for &str {
    #[inline(always)]
    unsafe fn span_from<'src>(
        this: &mut CacheOf<'src, Self>,
        range: RangeFrom<&CursorOf<'src, Self>>,
    ) -> Self::Span {
        (*range.start..this.len()).into()
    }
}

impl ValueInput for &str {
    #[inline(always)]
    unsafe fn next<'src>(
        cache: &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<char> {
        unsafe { Self::next_maybe(cache, cursor) }
    }
}

impl Sealed for &str {}
impl StrInput for &str {
    #[doc(hidden)]
    fn stringify<'src>(slice: SliceOf<'src, Self>) -> String {
        slice.to_string()
    }
}
impl<'src> SliceInputFor<'src> for &str {
    type Slice = Ref<'src, str>;
}

impl SliceInput for &str {
    #[inline(always)]
    fn full_slice<'src>(this: &mut CacheOf<'src, Self>) -> SliceOf<'src, Self> {
        (*this).into()
    }

    #[inline(always)]
    unsafe fn slice<'src>(
        this: &mut CacheOf<'src, Self>,
        range: Range<&CursorOf<'src, Self>>,
    ) -> SliceOf<'src, Self> {
        this[*range.start..*range.end].into()
    }

    #[inline(always)]
    unsafe fn slice_from<'src>(
        this: &mut CacheOf<'src, Self>,
        from: RangeFrom<&CursorOf<'src, Self>>,
    ) -> SliceOf<'src, Self> {
        this[*from.start..].into()
    }
}

impl<'src, T> InputFor<'src, T> for &[T] {
    type Cursor = usize;

    type MaybeToken = Ref<'src, T>;

    type Cache = Self;
}

impl<T> Input for &[T] {
    type Span = SimpleSpan<usize>;

    type Token = T;
    #[inline]
    fn begin<'src>(self) -> (CursorOf<'src, Self>, CacheOf<'src, Self>) {
        (0, self)
    }

    #[inline]
    fn cursor_location<'src>(cursor: &CursorOf<'src, Self>) -> usize {
        *cursor
    }

    #[inline(always)]
    unsafe fn next_maybe<'src>(
        this: &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<MaybeTokenOf<'src, Self>> {
        if let Some(tok) = this.get(*cursor) {
            *cursor += 1;
            Some(tok.into())
        } else {
            None
        }
    }

    #[inline(always)]
    unsafe fn span<'src>(
        _this: &mut CacheOf<'src, Self>,
        range: Range<&CursorOf<'src, Self>>,
    ) -> Self::Span {
        (*range.start..*range.end).into()
    }
}

impl<T> ExactSizeInput for &[T] {
    #[inline(always)]
    unsafe fn span_from<'src>(
        this: &mut CacheOf<'src, Self>,
        range: RangeFrom<&CursorOf<'src, Self>>,
    ) -> Self::Span {
        (*range.start..this.len()).into()
    }
}

impl Sealed for &[u8] {}
impl StrInput for &[u8] {
    #[doc(hidden)]
    fn stringify<'src>(slice: SliceOf<'src, Self>) -> String {
        slice
            .iter()
            // .map(|e| core::ascii::Char::from_u8(e).unwrap_or(AsciiChar::Substitute).to_char())
            .map(|e| char::from(*e))
            .collect()
    }
}
impl<'src, T> SliceInputFor<'src> for &[T] {
    type Slice = Ref<'src, [T]>;
}

impl<T> SliceInput for &[T] {
    #[inline(always)]
    fn full_slice<'src>(this: &mut CacheOf<'src, Self>) -> SliceOf<'src, Self>
    where
        Self: 'src,
    {
        (*this).into()
    }

    #[inline(always)]
    unsafe fn slice<'src>(
        this: &mut CacheOf<'src, Self>,
        range: Range<&CursorOf<'src, Self>>,
    ) -> SliceOf<'src, Self>
    where
        Self: 'src,
    {
        (&this[*range.start..*range.end]).into()
    }

    #[inline(always)]
    unsafe fn slice_from<'src>(
        this: &mut CacheOf<'src, Self>,
        from: RangeFrom<&CursorOf<'src, Self>>,
    ) -> SliceOf<'src, Self>
    where
        Self: 'src,
    {
        (&this[*from.start..]).into()
    }
}

impl<T: Clone> ValueInput for &[T] {
    #[inline(always)]
    unsafe fn next<'src>(
        this: &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<Self::Token> {
        unsafe {
            Self::next_maybe(this, cursor).map(|r| <Ref<'_, _> as Borrow<T>>::borrow(&r).clone())
        }
    }
}

impl<T> BorrowInput for &[T] {
    #[inline(always)]
    unsafe fn next_ref<'src>(
        this: &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<&'src Self::Token> {
        unsafe { Self::next_maybe(this, cursor).map(Ref::into_ref) }
    }
}

impl<'src, T, const N: usize> InputFor<'src, T> for &'_ [T; N] {
    type Cursor = usize;

    type MaybeToken = Ref<'src, T>;

    type Cache = Self;
}

impl<T, const N: usize> Input for &[T; N] {
    type Span = SimpleSpan<usize>;

    type Token = T;
    #[inline]
    fn begin<'src>(self) -> (CursorOf<'src, Self>, CacheOf<'src, Self>) {
        (0, self)
    }

    #[inline]
    fn cursor_location<'src>(cursor: &CursorOf<'src, Self>) -> usize {
        *cursor
    }

    #[inline(always)]
    unsafe fn next_maybe<'src>(
        this: &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<MaybeTokenOf<'src, Self>> {
        if let Some(tok) = this.get(*cursor) {
            *cursor += 1;
            Some(tok.into())
        } else {
            None
        }
    }

    #[inline(always)]
    unsafe fn span<'src>(
        _this: &mut CacheOf<'src, Self>,
        range: Range<&CursorOf<'src, Self>>,
    ) -> Self::Span {
        (*range.start..*range.end).into()
    }
}

impl<T, const N: usize> ExactSizeInput for &[T; N] {
    #[inline(always)]
    unsafe fn span_from<'src>(
        this: &mut CacheOf<'src, Self>,
        range: RangeFrom<&CursorOf<'src, Self>>,
    ) -> Self::Span {
        (*range.start..this.len()).into()
    }
}

impl<const N: usize> Sealed for &[u8; N] {}
impl<const N: usize> StrInput for &[u8; N] {
    #[doc(hidden)]
    fn stringify<'src>(slice: SliceOf<'src, Self>) -> String {
        slice
            .iter()
            // .map(|e| core::ascii::Char::from_u8(e).unwrap_or(AsciiChar::Substitute).to_char())
            .map(|e| char::from(*e))
            .collect()
    }
}

impl<'src, T, const N: usize> SliceInputFor<'src> for &[T; N] {
    type Slice = Ref<'src, [T]>;
}

impl<T, const N: usize> SliceInput for &[T; N] {
    #[inline(always)]
    fn full_slice<'src>(this: &mut CacheOf<'src, Self>) -> SliceOf<'src, Self>
    where
        Self: 'src,
    {
        Ref::from(&this[..])
    }

    #[inline(always)]
    unsafe fn slice<'src>(
        this: &mut CacheOf<'src, Self>,
        range: Range<&CursorOf<'src, Self>>,
    ) -> SliceOf<'src, Self>
    where
        Self: 'src,
    {
        Ref::from(&this[*range.start..*range.end])
    }

    #[inline(always)]
    unsafe fn slice_from<'src>(
        this: &mut CacheOf<'src, Self>,
        from: RangeFrom<&CursorOf<'src, Self>>,
    ) -> SliceOf<'src, Self>
    where
        Self: 'src,
    {
        Ref::from(&this[*from.start..])
    }
}

impl<T: Clone, const N: usize> ValueInput for &[T; N] {
    #[inline(always)]
    unsafe fn next<'src>(
        this: &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<Self::Token> {
        unsafe { Self::next_maybe(this, cursor).map(Ref::into_ref).cloned() }
    }
}

impl<T, const N: usize> BorrowInput for &[T; N] {
    #[inline(always)]
    unsafe fn next_ref<'src>(
        this: &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<&'src Self::Token> {
        unsafe { Self::next_maybe(this, cursor).map(Ref::into_ref) }
    }
}

pub trait InputMapper<'src, MaybeToken, OutTok, OutSpan>:
    Fn(
        MaybeToken,
        &'src (),
    ) -> (
        <Self as InputMapper<'src, MaybeToken, OutTok, OutSpan>>::OutputToken,
        <Self as InputMapper<'src, MaybeToken, OutTok, OutSpan>>::OutputSpan,
    ) + Sized
{
    type OutputToken;
    type OutputSpan;
}

impl<'src, F, MaybeToken, OutTok, OutSpan, OT, OS> InputMapper<'src, MaybeToken, OutTok, OutSpan>
    for F
where
    F: Fn(MaybeToken, &'src ()) -> (OT, OS),
{
    type OutputToken = OT;
    type OutputSpan = OS;
}

/// See [`Input::map`].
#[derive(Clone, Copy)]
pub struct MappedInput<
    I: Input,
    T,
    S,
    M = for<'src> fn(
        MaybeTokenOf<'src, I>,
        &'src (),
    ) -> (
        <MaybeTokenOf<'src, I> as IntoMaybe<'src, <I as Input>::Token>>::Proj<T>,
        <MaybeTokenOf<'src, I> as IntoMaybe<'src, <I as Input>::Token>>::Proj<S>,
    ),
> {
    input: I,
    eoi: S,
    mapper: M,
    _tok: EmptyPhantom<T>,
}

impl<'src, I, M, T, S: Span> InputFor<'src, T> for MappedInput<I, T, S, M>
where
    I: Input,
    M: for<'any> InputMapper<'any, MaybeTokenOf<'any, I>, T, S>,
{
    type MaybeToken = <MaybeTokenOf<'src, I> as IntoMaybe<'src, I::Token>>::Proj<T>;

    type Cursor = (CursorOf<'src, I>, Option<S::Offset>);
    type Cache = (CacheOf<'src, I>, M, S);
}

impl<I, T, S, F> Input for MappedInput<I, T, S, F>
where
    I: Input,
    F: for<'any> InputMapper<
            'any,
            MaybeTokenOf<'any, I>,
            T,
            S,
            OutputToken = <MaybeTokenOf<'any, I> as IntoMaybe<'any, I::Token>>::Proj<T>,
            OutputSpan = <MaybeTokenOf<'any, I> as IntoMaybe<'any, I::Token>>::Proj<S>,
        >,
    S: Span,
{
    type Span = S;

    type Token = T;
    #[inline]
    fn begin<'src>(self) -> (CursorOf<'src, Self>, CacheOf<'src, Self>) {
        let (cursor, cache) = self.input.begin();
        ((cursor, None), (cache, self.mapper, self.eoi))
    }

    #[inline]
    fn cursor_location<'src>(cursor: &CursorOf<'src, Self>) -> usize {
        I::cursor_location(&cursor.0)
    }

    unsafe fn next_maybe<'src>(
        (cache, mapper, _): &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<MaybeTokenOf<'src, Self>> {
        unsafe {
            I::next_maybe(cache, &mut cursor.0).map(|tok| {
                let (tok, span) = mapper(tok, &());
                cursor.1 = Some(span.borrow().end());
                tok
            })
        }
    }

    #[inline]
    unsafe fn span<'src>(
        (cache, mapper, eoi): &mut CacheOf<'src, Self>,
        range: Range<&CursorOf<'src, Self>>,
    ) -> Self::Span {
        match unsafe { I::next_maybe(cache, &mut range.start.0.clone()) } {
            Some(tok) => {
                let start = mapper(tok, &()).1.borrow().start();
                let end = range.end.1.clone().unwrap_or_else(|| eoi.end());
                S::new(eoi.context(), start..end)
            }
            None => S::new(eoi.context(), eoi.end()..eoi.end()),
        }
    }
}

impl<T, S, I, M> ExactSizeInput for MappedInput<I, T, S, M>
where
    I: ExactSizeInput,
    S: Span + Clone,
    M: for<'src> InputMapper<
            'src,
            MaybeTokenOf<'src, I>,
            T,
            S,
            OutputToken = <MaybeTokenOf<'src, I> as IntoMaybe<'src, I::Token>>::Proj<T>,
            OutputSpan = <MaybeTokenOf<'src, I> as IntoMaybe<'src, I::Token>>::Proj<S>,
        >,
{
    #[inline(always)]
    unsafe fn span_from<'src>(
        (cache, mapper, eoi): &mut CacheOf<'src, Self>,
        range: RangeFrom<&CursorOf<'src, Self>>,
    ) -> Self::Span {
        let start = unsafe {
            I::next_maybe(cache, &mut range.start.0.clone())
                .map(|tok| mapper(tok, &()).1.borrow().start())
                .unwrap_or_else(|| eoi.end())
        };
        S::new(eoi.context(), start..eoi.end())
    }
}

impl<T, S, I, M> ValueInput for MappedInput<I, T, S, M>
where
    I: ValueInput,
    S: Span + Clone,
    M: for<'src> InputMapper<
            'src,
            MaybeTokenOf<'src, I>,
            T,
            S,
            OutputToken = <MaybeTokenOf<'src, I> as IntoMaybe<'src, I::Token>>::Proj<T>,
            OutputSpan = <MaybeTokenOf<'src, I> as IntoMaybe<'src, I::Token>>::Proj<S>,
        >,
    S: Span,
    T: Clone,
{
    #[inline(always)]
    unsafe fn next<'src>(
        (cache, mapper, _): &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<Self::Token> {
        unsafe {
            I::next_maybe(cache, &mut cursor.0).map(|tok| {
                let (tok, span) = mapper(tok, &());
                cursor.1 = Some(span.borrow().end());
                tok.borrow().clone()
            })
        }
    }
}

impl<T, S, I, M> BorrowInput for MappedInput<I, T, S, M>
where
    I: Input + BorrowInput,
    for<'src> MaybeTokenOf<'src, I>: From<Ref<'src, I::Token>>,
    for<'src> MaybeTokenOf<'src, Self>: Into<Ref<'src, Self::Token>>,
    S: Span + Clone,
    M: for<'src> InputMapper<
            'src,
            MaybeTokenOf<'src, I>,
            T,
            S,
            OutputToken = <MaybeTokenOf<'src, I> as IntoMaybe<'src, I::Token>>::Proj<T>,
            OutputSpan = <MaybeTokenOf<'src, I> as IntoMaybe<'src, I::Token>>::Proj<S>,
        >,
{
    #[inline(always)]
    unsafe fn next_ref<'src>(
        (cache, mapper, _): &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<&'src Self::Token> {
        unsafe {
            I::next_ref(cache, &mut cursor.0).map(|tok| {
                let (tok, span) = mapper(Ref::from(tok).into(), &());
                cursor.1 = Some(span.borrow().end());
                tok.into().into_ref()
            })
        }
    }
}
// impl<'src, T, S, I, M> SliceInputFor<'src> for MappedInput<I, T, S, M>
// where
//     S: Span,
//     I: SliceInput,
//     M: for<'any> InputMapper<'any, MaybeTokenOf<'any, I>, T, S>,
// {
//     type Slice = SliceOf<'src, I>;
// }

// impl<T, S, I, M> SliceInput for MappedInput<I, T, S, M>
// where
//     I:  SliceInput<Token = (T, S)>,
//     S: Span + Clone,
//     M: for<'src> InputMapper<
//             'src,
//             MaybeTokenOf<'src, I>,
//             T,
//             S,
//             OutputToken = <MaybeTokenOf<'src, I> as IntoMaybe<'src,I::Token>>::Proj<T>,
//             OutputSpan = <MaybeTokenOf<'src, I> as IntoMaybe<'src,I::Token>>::Proj<S>,
//         >,
// {
//     #[inline(always)]
//     fn full_slice<'src>((cache, _, _): &mut CacheOf<'src, Self>) -> SliceOf<'src,Self> {
//         I::full_slice(cache)
//     }

//     #[inline(always)]
//     unsafe fn slice<'src>(
//         (cache, _, _): &mut CacheOf<'src, Self>,
//         range: Range<&CursorOf<'src, Self>>,
//     ) -> SliceOf<'src,Self> {
//         unsafe { I::slice(cache, &range.start.0..&range.end.0) }
//     }

//     #[inline(always)]
//     unsafe fn slice_from<'src>(
//         (cache, _, _): &mut CacheOf<'src, Self>,
//         from: RangeFrom<&CursorOf<'src, Self>>,
//     ) -> SliceOf<'src,Self> {
//         unsafe { I::slice_from(cache, &from.start.0..) }
//     }
// }

pub trait SpanMapper<'src, InSpan, OutSpan>: Fn(InSpan) -> OutSpan {
    type Output;
}

impl<'src, F, InSpan, OutSpan> SpanMapper<'src, InSpan, OutSpan> for F
where
    F: Fn(InSpan) -> OutSpan,
{
    type Output = OutSpan;
}

/// An input wrapper that maps the span type of your input
/// into your custom span [`Input::map_span`].
#[derive(Copy, Clone)]
pub struct MappedSpan<S: Span, I, F> {
    input: I,
    map_fn: F,
    phantom: PhantomData<S>,
}

impl<'src, I, S, F> InputFor<'src, I::Token> for MappedSpan<S, I, F>
where
    I: Input,
    F: for<'any> SpanMapper<'any, I::Span, S>,
    S: Span,
{
    type MaybeToken = MaybeTokenOf<'src, I>;

    type Cursor = CursorOf<'src, I>;

    type Cache = (CacheOf<'src, I>, F);
}

impl<S, I: Input, F> Input for MappedSpan<S, I, F>
where
    S: Span + Clone,
    S::Context: Clone,
    S::Offset: for<'src> From<<I::Span as Span>::Offset>,
    F: for<'any> SpanMapper<'any, I::Span, S>,
{
    type Span = S;

    type Token = I::Token;
    #[inline(always)]
    fn begin<'src>(self) -> (CursorOf<'src, Self>, CacheOf<'src, Self>) {
        let (cursor, cache) = self.input.begin();
        (cursor, (cache, self.map_fn))
    }

    #[inline]
    fn cursor_location<'src>(cursor: &CursorOf<'src, Self>) -> usize {
        I::cursor_location(cursor)
    }

    #[inline(always)]
    unsafe fn next_maybe<'src>(
        (cache, _): &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<MaybeTokenOf<'src, Self>> {
        unsafe { I::next_maybe(cache, cursor) }
    }

    #[inline]
    unsafe fn span<'src>(
        (cache, mapper): &mut CacheOf<'src, Self>,
        range: Range<&CursorOf<'src, Self>>,
    ) -> Self::Span {
        let inner_span = unsafe { I::span(cache, range) };
        mapper(inner_span)
    }
}

impl<S, I: Input, F> ExactSizeInput for MappedSpan<S, I, F>
where
    I: ExactSizeInput,
    S: Span + Clone,
    S::Context: Clone,
    for<'any> S::Offset: From<<I::Span as Span>::Offset>,
    F: for<'any> SpanMapper<'any, I::Span, S>,
{
    #[inline(always)]
    unsafe fn span_from<'src>(
        (cache, mapper): &mut CacheOf<'src, Self>,
        range: RangeFrom<&CursorOf<'src, Self>>,
    ) -> Self::Span {
        let inner_span = unsafe { I::span_from(cache, range) };
        mapper(inner_span)
    }
}

impl<S, I: ValueInput, F> ValueInput for MappedSpan<S, I, F>
where
    S: Span + Clone,
    S::Context: Clone,
    for<'any> S::Offset: From<<I::Span as Span>::Offset>,
    F: for<'any> SpanMapper<'any, I::Span, S>,
{
    #[inline(always)]
    unsafe fn next<'src>(
        (cache, _): &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<Self::Token> {
        unsafe { I::next(cache, cursor) }
    }
}

impl<S, I: BorrowInput, F> BorrowInput for MappedSpan<S, I, F>
where
    S: Span + Clone,
    S::Context: Clone,
    for<'any> S::Offset: From<<I::Span as Span>::Offset>,
    F: for<'any> SpanMapper<'any, I::Span, S>,
{
    #[inline(always)]
    unsafe fn next_ref<'src>(
        (cache, _): &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<&'src Self::Token> {
        unsafe { I::next_ref(cache, cursor) }
    }
}

impl<'src, S: Span, I: SliceInput, F> SliceInputFor<'src> for MappedSpan<S, I, F> {
    type Slice = SliceOf<'src, I>;
}

impl<S, I, F> SliceInput for MappedSpan<S, I, F>
where
    I: SliceInput,
    S: Span + Clone,
    S::Context: Clone,
    F: for<'any> SpanMapper<'any, I::Span, S>,
    S::Offset: From<<I::Span as Span>::Offset>,
{
    #[inline(always)]
    fn full_slice<'src>((cache, _): &mut CacheOf<'src, Self>) -> SliceOf<'src, Self>
    where
        Self: 'src,
    {
        I::full_slice(cache)
    }

    #[inline(always)]
    unsafe fn slice<'src>(
        (cache, _): &mut CacheOf<'src, Self>,
        range: Range<&CursorOf<'src, Self>>,
    ) -> SliceOf<'src, Self>
    where
        Self: 'src,
    {
        unsafe { I::slice(cache, range) }
    }

    #[inline(always)]
    unsafe fn slice_from<'src>(
        (cache, _): &mut CacheOf<'src, Self>,
        from: RangeFrom<&CursorOf<'src, Self>>,
    ) -> SliceOf<'src, Self>
    where
        Self: 'src,
    {
        unsafe { I::slice_from(cache, from) }
    }
}

impl<'src, S, I, F: 'src> Sealed for MappedSpan<S, I, F>
where
    I: Input,
    S: Span + Clone,
    S::Context: Clone,
    for<'any> S::Offset: From<<I::Span as Span>::Offset>,
    F: for<'any> SpanMapper<'any, I::Span, S>,
{
}
impl<S, I, F> StrInput for MappedSpan<S, I, F>
where
    I: StrInput,
    S: Span + Clone,
    S::Context: Clone,
    I::Token: Char,
    S::Offset: From<<I::Span as Span>::Offset>,
    F: for<'any> SpanMapper<'any, I::Span, S>,
{
    #[doc(hidden)]
    fn stringify<'src>(slice: SliceOf<'src, Self>) -> String {
        I::stringify(slice)
    }
}

/// An input wrapper that returns a custom span, with the user-defined context
/// contained in the Span::Context. See [`Input::with_context`].
#[derive(Copy, Clone)]
pub struct WithContext<S: Span, I> {
    input: I,
    context: S::Context,
    #[allow(dead_code)]
    phantom: EmptyPhantom<S>,
}

impl<'src, I: Input, S: Span> InputFor<'src, I::Token> for WithContext<S, I> {
    type Cursor = CursorOf<'src, I>;

    type MaybeToken = MaybeTokenOf<'src, I>;

    type Cache = (CacheOf<'src, I>, S::Context);
}

impl<S, I: Input> Input for WithContext<S, I>
where
    S: Span + Clone,
    S::Context: Clone,
    S::Offset: From<<I::Span as Span>::Offset>,
{
    type Span = S;

    type Token = I::Token;

    #[inline(always)]
    fn begin<'src>(self) -> (CursorOf<'src, Self>, CacheOf<'src, Self>) {
        let (cursor, cache) = self.input.begin();
        (cursor, (cache, self.context))
    }

    #[inline]
    fn cursor_location<'src>(cursor: &CursorOf<'src, Self>) -> usize {
        I::cursor_location(cursor)
    }

    #[inline(always)]
    unsafe fn next_maybe<'src>(
        (cache, _): &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<MaybeTokenOf<'src, Self>> {
        unsafe { I::next_maybe(cache, cursor) }
    }

    #[inline]
    unsafe fn span<'src>(
        (cache, ctx): &mut CacheOf<'src, Self>,
        range: Range<&CursorOf<'src, Self>>,
    ) -> Self::Span {
        let inner_span = unsafe { I::span(cache, range) };
        S::new(
            ctx.clone(),
            inner_span.start().into()..inner_span.end().into(),
        )
    }
}

impl<S, I: Input> ExactSizeInput for WithContext<S, I>
where
    I: ExactSizeInput,
    S: Span + Clone,
    S::Context: Clone,
    S::Offset: for<'src> From<<I::Span as Span>::Offset>,
{
    #[inline]
    unsafe fn span_from<'src>(
        (cache, ctx): &mut CacheOf<'src, Self>,
        range: RangeFrom<&CursorOf<'src, Self>>,
    ) -> Self::Span {
        let inner_span = unsafe { I::span_from(cache, range) };
        S::new(
            ctx.clone(),
            inner_span.start().into()..inner_span.end().into(),
        )
    }
}

impl<S, I: ValueInput> ValueInput for WithContext<S, I>
where
    S: Span + Clone,
    S::Context: Clone,
    for<'src> S::Offset: From<<I::Span as Span>::Offset>,
{
    #[inline(always)]
    unsafe fn next<'src>(
        (cache, _): &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<Self::Token> {
        unsafe { I::next(cache, cursor) }
    }
}

impl<S, I: BorrowInput> BorrowInput for WithContext<S, I>
where
    S: Span + Clone,
    S::Context: Clone,
    for<'src> S::Offset: From<<I::Span as Span>::Offset>,
{
    #[inline(always)]
    unsafe fn next_ref<'src>(
        (cache, _): &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<&'src Self::Token> {
        unsafe { I::next_ref(cache, cursor) }
    }
}

impl<'src, S: Span, I: SliceInput> SliceInputFor<'src> for WithContext<S, I> {
    type Slice = SliceOf<'src, I>;
}

impl<S, I: SliceInput> SliceInput for WithContext<S, I>
where
    S: Span + Clone,
    S::Context: Clone,
    S::Offset: From<<I::Span as Span>::Offset>,
{
    #[inline(always)]
    fn full_slice<'src>((cache, _): &mut CacheOf<'src, Self>) -> SliceOf<'src, Self>
    where
        Self: 'src,
    {
        I::full_slice(cache)
    }

    #[inline(always)]
    unsafe fn slice<'src>(
        (cache, _): &mut CacheOf<'src, Self>,
        range: Range<&CursorOf<'src, Self>>,
    ) -> SliceOf<'src, Self>
    where
        Self: 'src,
    {
        unsafe { I::slice(cache, range) }
    }

    #[inline(always)]
    unsafe fn slice_from<'src>(
        (cache, _): &mut CacheOf<'src, Self>,
        from: RangeFrom<&CursorOf<'src, Self>>,
    ) -> SliceOf<'src, Self>
    where
        Self: 'src,
    {
        unsafe { I::slice_from(cache, from) }
    }
}

impl<S: Span, I> Sealed for WithContext<S, I> {}

impl<S, I> StrInput for WithContext<S, I>
where
    I: StrInput,
    S: Span + Clone,
    S::Context: Clone,
    I::Token: Char,
    S::Offset: From<<I::Span as Span>::Offset>,
{
    #[doc(hidden)]
    fn stringify<'src>(slice: SliceOf<'src, Self>) -> String {
        I::stringify(slice)
    }
}

/// Input type which supports seekable readers. Uses a [`BufReader`] internally to buffer input and
/// avoid unnecessary IO calls.
///
/// Only available with the `std` feature
#[cfg(feature = "std")]
pub struct IoInput<R> {
    reader: BufReader<R>,
    last_cursor: usize,
}

#[cfg(feature = "std")]
impl<R: Read + Seek> IoInput<R> {
    /// Create a new `IoReader` from a seekable reader.
    pub fn new(reader: R) -> IoInput<R> {
        IoInput {
            reader: BufReader::new(reader),
            last_cursor: 0,
        }
    }
}

impl<R: Read + Seek> InputFor<'_, u8> for IoInput<R> {
    type Cursor = usize;

    type MaybeToken = u8;

    type Cache = Self;
}

#[cfg(feature = "std")]
impl<R: Read + Seek> Input for IoInput<R> {
    type Span = SimpleSpan;

    type Token = u8;
    fn begin<'src>(self) -> (CursorOf<'src, Self>, CacheOf<'src, Self>) {
        (0, self)
    }

    #[inline(always)]
    fn cursor_location<'src>(cursor: &CursorOf<'src, Self>) -> usize {
        *cursor
    }

    #[inline(always)]
    unsafe fn next_maybe<'src>(
        this: &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<MaybeTokenOf<'src, Self>> {
        unsafe { Self::next(this, cursor) }
    }

    #[inline]
    unsafe fn span<'src>(
        _this: &mut CacheOf<'src, Self>,
        range: Range<&CursorOf<'src, Self>>,
    ) -> Self::Span {
        (*range.start..*range.end).into()
    }
}

#[cfg(feature = "std")]
impl<R: Read + Seek> ValueInput for IoInput<R> {
    unsafe fn next<'src>(
        this: &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<u8> {
        if *cursor != this.last_cursor {
            let seek = *cursor as i64 - this.last_cursor as i64;

            this.reader.seek_relative(seek).unwrap();

            this.last_cursor = *cursor;
        }

        let mut out = 0;

        let r = this.reader.read_exact(std::slice::from_mut(&mut out));

        match r {
            Ok(()) => {
                this.last_cursor += 1;
                *cursor += 1;
                Some(out)
            }
            Err(_) => None,
        }
    }
}

/// Represents a location in an input that can be rewound to.
///
/// Checkpoints can be created with [`InputRef::save`] and rewound to with [`InputRef::rewind`].
pub struct Checkpoint<'src, 'parse, I: Input, C> {
    cursor: Cursor<'src, 'parse, I>,
    pub(crate) err_count: usize,
    pub(crate) inspector: C,
    phantom: PhantomData<fn(&'parse ()) -> &'parse ()>, // Invariance
}

impl<'src, 'parse, I: Input, C> Checkpoint<'src, 'parse, I, C> {
    /// Get the [`Cursor`] that this checkpoint corresponds to.
    pub fn cursor(&self) -> &Cursor<'src, 'parse, I> {
        &self.cursor
    }

    /// Get the [`Checkpoint`][Inspector::Checkpoint] that this marker corresponds to.
    pub fn inspector(&self) -> &C {
        &self.inspector
    }
}

impl<'src, I: Input, C: Clone> Clone for Checkpoint<'src, '_, I, C> {
    #[inline(always)]
    fn clone(&self) -> Self {
        Self {
            cursor: self.cursor.clone(),
            err_count: self.err_count,
            inspector: self.inspector.clone(),
            phantom: PhantomData,
        }
    }
}

/// Represents a location in an input.
///
/// If you to rewind to an old input location, see [`Checkpoint`].
#[repr(transparent)]
pub struct Cursor<'src, 'parse, I: Input> {
    pub(crate) inner: CursorOf<'src, I>,
    phantom: PhantomData<fn(&'parse ()) -> &'parse ()>, // Invariance
}

impl<'src, I: Input> Cursor<'src, '_, I> {
    /// Get the input's internal cursor.
    pub fn inner(&self) -> &CursorOf<'src, I> {
        &self.inner
    }
}

impl<I: Input> Clone for Cursor<'_, '_, I> {
    #[inline(always)]
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            phantom: PhantomData,
        }
    }
}

impl<I: Input> Eq for Cursor<'_, '_, I> {}
impl<I: Input> PartialEq for Cursor<'_, '_, I> {
    fn eq(&self, other: &Self) -> bool {
        I::cursor_location(&self.inner)
            .cmp(&I::cursor_location(&other.inner))
            .is_eq()
    }
}

impl<I: Input> PartialOrd for Cursor<'_, '_, I> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<I: Input> Ord for Cursor<'_, '_, I> {
    fn cmp(&self, other: &Self) -> Ordering {
        I::cursor_location(&self.inner).cmp(&I::cursor_location(&other.inner))
    }
}

pub(crate) struct Errors<T, E> {
    pub(crate) alt: Option<Located<T, E>>,
    pub(crate) secondary: Vec<Located<T, E>>,
}

impl<T, E> Errors<T, E> {
    /// Returns a slice of the secondary errors (if any) have been emitted since the given checkpoint was created.
    #[inline]
    pub(crate) fn secondary_errors_since(&mut self, err_count: usize) -> &mut [Located<T, E>] {
        self.secondary.get_mut(err_count..).unwrap_or(&mut [])
    }
}

impl<T, E> Default for Errors<T, E> {
    fn default() -> Self {
        Self {
            alt: None,
            secondary: Vec::new(),
        }
    }
}

/// Internal type representing the owned parts of an input - used at the top level by a call to
/// `parse`.
pub(crate) struct InputOwn<'src, 's, I: Input, E: ParserExtra<I>> {
    pub(crate) start: CursorOf<'src, I>,
    pub(crate) cache: CacheOf<'src, I>,
    pub(crate) errors: Errors<CursorOf<'src, I>, ErrOfEx<'src, I, E>>,
    pub(crate) state: MaybeMut<'s, E::State>,
    pub(crate) ctx: CtxOf<'src, I, E>,
    #[cfg(feature = "memoization")]
    pub(crate) memos:
        HashMap<(usize, usize), Option<Located<CursorOf<'src, I>, ErrOfEx<'src, I, E>>>>,
}

impl<'src, 's, I, E> InputOwn<'src, 's, I, E>
where
    I: Input,
    E: ParserExtra<I>,
    I::Token: IntoMaybe<'src, I::Token>,
{
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn new(input: I) -> InputOwn<'src, 's, I, E>
    where
        E::State: Default,
        CtxOf<'src, I, E>: Default,
    {
        let (start, cache) = input.begin();
        InputOwn {
            start,
            cache,
            errors: Errors::default(),
            state: MaybeMut::Val(E::State::default()),
            ctx: CtxOf::<'src, I, E>::default(),
            #[cfg(feature = "memoization")]
            memos: HashMap::default(),
        }
    }

    pub(crate) fn new_state(input: I, state: &'s mut E::State) -> InputOwn<'src, 's, I, E>
    where
        CtxOf<'src, I, E>: Default,
    {
        let (start, cache) = input.begin();
        InputOwn {
            start,
            cache,
            errors: Errors::default(),
            state: MaybeMut::Ref(state),
            ctx: CtxOf::<'src, I, E>::default(),
            #[cfg(feature = "memoization")]
            memos: HashMap::default(),
        }
    }

    pub(crate) fn as_ref_start<'parse>(&'parse mut self) -> InputRef<'src, 'parse, I, E> {
        InputRef {
            cursor: self.start.clone(),
            cache: &mut self.cache,
            errors: &mut self.errors,
            state: &mut self.state,
            ctx: &self.ctx,
            #[cfg(feature = "memoization")]
            memos: &mut self.memos,
        }
    }

    pub(crate) fn into_errs(self) -> Vec<ErrOfEx<'src, I, E>> {
        self.errors
            .secondary
            .into_iter()
            .map(|err| err.err)
            .collect()
    }
}

/// Internal type representing an input as well as all the necessary context for parsing.
pub struct InputRef<'src, 'parse, I: Input, E: ParserExtra<I>> {
    cursor: CursorOf<'src, I>,
    pub(crate) cache: &'parse mut CacheOf<'src, I>,
    pub(crate) errors: &'parse mut Errors<CursorOf<'src, I>, ErrOfEx<'src, I, E>>,
    pub(crate) state: &'parse mut E::State,
    pub(crate) ctx: &'parse CtxOf<'src, I, E>,
    #[cfg(feature = "memoization")]
    pub(crate) memos: &'parse mut HashMap<
        (usize, usize),
        Option<Located<CursorOf<'src, I>, ErrOfEx<'src, I, E>>>,
    >,
}

impl<'src, 'parse, I: Input, E: ParserExtra<I>> InputRef<'src, 'parse, I, E>
where
    I::Token: IntoMaybe<'src, I::Token>,
{
    #[inline]
    pub(crate) fn with_ctx<'sub_parse, EM, O>(
        &'sub_parse mut self,
        new_ctx: &'sub_parse <EM::ContextFam as Hkt>::Of<'src>,
        f: impl FnOnce(&mut InputRef<'src, 'sub_parse, I, EM>) -> O,
    ) -> O
    where
        'parse: 'sub_parse,
        EM: ParserExtra<I, ErrorFam = E::ErrorFam, State = E::State>,
    {
        let mut new_inp = InputRef {
            cursor: self.cursor.clone(),
            cache: self.cache,
            state: self.state,
            ctx: new_ctx,
            errors: self.errors,
            #[cfg(feature = "memoization")]
            memos: self.memos,
        };
        let res = f(&mut new_inp);
        self.cursor = new_inp.cursor;
        res
    }

    #[inline]
    pub(crate) fn with_state<'sub_parse, S, O>(
        &'sub_parse mut self,
        new_state: &'sub_parse mut S,
        f: impl FnOnce(
            &mut InputRef<'src, 'sub_parse, I, extra::Full<E::ErrorFam, S, E::ContextFam>>,
        ) -> O,
    ) -> O
    where
        'parse: 'sub_parse,
        S: Inspector<I>,
    {
        let mut new_inp = InputRef {
            cursor: self.cursor.clone(),
            cache: self.cache,
            state: new_state,
            ctx: self.ctx,
            errors: self.errors,
            #[cfg(feature = "memoization")]
            memos: self.memos,
        };
        let res = f(&mut new_inp);
        self.cursor = new_inp.cursor;
        res
    }

    #[inline]
    pub(crate) fn with_input<'sub_parse, J, F, O>(
        &'sub_parse mut self,
        start: CursorOf<'src, J>,
        cache: &'sub_parse mut CacheOf<'src, J>,
        new_errors: &'sub_parse mut Errors<CursorOf<'src, J>, ErrOfEx<'src, J, F>>,
        f: impl FnOnce(&mut InputRef<'src, 'sub_parse, J, F>) -> O,
        #[cfg(feature = "memoization")] memos: &'sub_parse mut HashMap<
            (usize, usize),
            Option<Located<CursorOf<'src, J>, ErrOfEx<'src, J, F>>>,
        >,
    ) -> O
    where
        'parse: 'sub_parse,
        J: Input,
        F: ParserExtra<J, State = E::State, ContextFam = E::ContextFam, ErrorFam = E::ErrorFam>,
        J: Input<Token = I::Token, Span = I::Span>,
    {
        let mut new_inp = InputRef {
            cursor: start,
            cache,
            state: self.state,
            ctx: self.ctx,
            errors: new_errors,
            #[cfg(feature = "memoization")]
            memos,
        };
        let out = f(&mut new_inp);
        self.errors.secondary.extend(
            new_inp
                .errors
                .secondary
                .drain(..)
                .map(|err| Located::at(self.cursor.clone(), err.err)),
        );
        if let Some(alt) = new_inp.errors.alt.take() {
            self.errors.alt = Some(Located::at(self.cursor.clone(), alt.err));
        }
        out
    }

    /// Get the internal cursor of the input at this moment in time.
    ///
    /// Can be used for generating spans or slices. See [`InputRef::span_from`] and [`InputRef::slice`].
    #[inline(always)]
    pub fn cursor(&self) -> Cursor<'src, 'parse, I> {
        Cursor {
            // TODO: Find ways to avoid this clone, if possible
            inner: self.cursor.clone(),
            phantom: PhantomData,
        }
    }

    /// Save the current parse state as a [`Checkpoint`].
    ///
    /// You can rewind back to this state later with [`InputRef::rewind`].
    #[inline(always)]
    pub fn save(&self) -> Checkpoint<'src, 'parse, I, <E::State as Inspector<I>>::Checkpoint> {
        let cursor = self.cursor();
        let inspector = self.state.on_save(&cursor);
        Checkpoint {
            cursor,
            err_count: self.errors.secondary.len(),
            inspector,
            phantom: PhantomData,
        }
    }

    /// Reset the parse state to that represented by the given [`Checkpoint`].
    ///
    /// You can create a checkpoint with which to perform rewinding using [`InputRef::save`].
    #[inline(always)]
    pub fn rewind(
        &mut self,
        checkpoint: Checkpoint<'src, 'parse, I, <E::State as Inspector<I>>::Checkpoint>,
    ) {
        self.errors.secondary.truncate(checkpoint.err_count);
        self.state.on_rewind(&checkpoint);
        self.cursor = checkpoint.cursor.inner;
    }

    #[inline(always)]
    pub(crate) fn rewind_input(
        &mut self,
        checkpoint: Checkpoint<'src, 'parse, I, <E::State as Inspector<I>>::Checkpoint>,
    ) {
        self.cursor = checkpoint.cursor.inner;
    }

    /// Get a mutable reference to the state associated with the current parse.
    #[inline(always)]
    pub fn state(&mut self) -> &mut E::State {
        self.state
    }

    /// Get a reference to the context fed to the current parser.
    ///
    /// See [`ConfigParser::configure`], [`Parser::ignore_with_ctx`] and
    /// [`Parser::then_with_ctx`] for more information about context-sensitive
    /// parsing.
    #[inline(always)]
    pub fn ctx(&self) -> &CtxOf<'src, I, E> {
        self.ctx
    }

    #[inline]
    pub(crate) fn skip_while<F: FnMut(&I::Token) -> bool>(&mut self, mut f: F)
    where
        I: Input,
    {
        loop {
            let mut cursor = self.cursor.clone();
            // SAFETY: cursor was generated by previous call to `Input::next`
            let token = unsafe { I::next_maybe(self.cache, &mut cursor) };
            if token.as_ref().filter(|tok| f((*tok).borrow())).is_none() {
                break;
            } else {
                if let Some(t) = &token {
                    self.state.on_token(t.borrow());
                }
                self.cursor = cursor;
            }
        }
    }

    #[inline(always)]
    pub(crate) fn next_inner(&mut self) -> Option<I::Token>
    where
        I: ValueInput,
    {
        // SAFETY: cursor was generated by previous call to `Input::next`
        let token = unsafe { I::next(self.cache, &mut self.cursor) };
        if let Some(t) = &token {
            self.state.on_token(t);
        }
        token
    }

    #[inline(always)]
    pub(crate) fn next_maybe_inner(&mut self) -> Option<MaybeTokenOf<'src, I>> {
        // SAFETY: cursor was generated by previous call to `Input::next`
        let token = unsafe { I::next_maybe(self.cache, &mut self.cursor) };
        if let Some(t) = &token {
            self.state.on_token(t.borrow());
        }
        token
    }

    #[inline(always)]
    pub(crate) fn next_ref_inner(&mut self) -> Option<&'src I::Token>
    where
        I: BorrowInput,
    {
        // SAFETY: cursor was generated by previous call to `Input::next`
        let token = unsafe { I::next_ref(self.cache, &mut self.cursor) };
        if let Some(t) = &token {
            self.state.on_token(t);
        }
        token
    }

    /// Attempt to parse this input using the given parser.
    ///
    /// # Important Notice
    ///
    /// Parsers that return `Err(...)` are permitted to leave the input in an **unspecified** (but not
    /// [undefined](https://en.wikipedia.org/wiki/Undefined_behavior)) state.
    ///
    /// The only well-specified action you are permitted to perform on the input after an error has occurred is
    /// rewinding to a checkpoint created *before* the error occurred via [`InputRef::rewind`].
    ///
    /// This state is not consistent between releases of chumsky, compilations of the final binary, or even invocations
    /// of the parser. You should not rely on this state for anything, and choosing to rely on it means that your
    /// parser may break in unexpected ways at any time.
    ///
    /// You have been warned.
    pub fn parse<O: Hkt, P: Parser<I, O, E>>(
        &mut self,
        parser: P,
    ) -> Result<O::Of<'src>, ErrOfEx<'src, I, E>> {
        match parser.go::<EmitRecover>(self) {
            Ok(out) => Ok(out),
            // Can't fail!
            Err(()) => Err(self.take_alt().unwrap().err),
        }
    }

    /// A check-only version of [`InputRef::parse`].
    ///
    /// # Import Notice
    ///
    /// See [`InputRef::parse`] about unspecified behavior associated with this function.
    pub fn check<O, P>(&mut self, parser: P) -> Result<(), ErrOfEx<'src, I, E>>
    where
        O: Hkt,
        P: Parser<I, O, E>,
    {
        match parser.go::<CheckRecover>(self) {
            Ok(()) => Ok(()),
            // Can't fail!
            Err(()) => Err(self.take_alt().unwrap().err),
        }
    }

    /// Get the next token in the input. Returns `None` if the end of the input has been reached.
    ///
    /// This function is more flexible than either [`InputRef::next`] or [`InputRef::next_ref`] since it
    /// only requires that the [`Input`] trait be implemented for `I` (instead of either [`ValueInput`] or
    /// [`BorrowInput`]). However, that increased flexibility for the end user comes with a trade-off for the
    /// implementation: this function returns a [`MaybeRef<TokenOf<'src,I>>`] that provides only a temporary reference to the
    /// token.
    ///
    /// See [`InputRef::next_ref`] if you want get a reference to the next token instead.
    #[inline(always)]
    pub fn next_maybe(&mut self) -> Option<MaybeRef<'src, I::Token>> {
        self.next_maybe_inner().map(Into::into)
    }

    /// Get the next token in the input by value. Returns `None` if the end of the input has been reached.
    ///
    /// See [`InputRef::next_ref`] if you want get a reference to the next token instead.
    #[inline(always)]
    pub fn next(&mut self) -> Option<I::Token>
    where
        I: ValueInput,
    {
        self.next_inner()
    }

    /// Get a reference to the next token in the input. Returns `None` if the end of the input has been reached.
    ///
    /// See [`InputRef::next`] if you want get the next token by value instead.
    #[inline(always)]
    pub fn next_ref(&mut self) -> Option<&'src I::Token>
    where
        I: BorrowInput,
    {
        self.next_ref_inner()
    }

    /// Peek the next token in the input. Returns `None` if the end of the input has been reached.
    ///
    /// See [`InputRef::next_maybe`] for more information about what this function guarantees.
    #[inline(always)]
    pub fn peek_maybe(&mut self) -> Option<MaybeRef<'src, I::Token>> {
        // SAFETY: cursor was generated by previous call to `Input::next`
        unsafe { I::next_maybe(self.cache, &mut self.cursor.clone()).map(Into::into) }
    }

    /// Peek the next token in the input. Returns `None` if the end of the input has been reached.
    #[inline(always)]
    pub fn peek(&mut self) -> Option<I::Token>
    where
        I: ValueInput,
    {
        // SAFETY: cursor was generated by previous call to `Input::next`
        unsafe { I::next(self.cache, &mut self.cursor.clone()) }
    }

    /// Peek the next token in the input. Returns `None` if the end of the input has been reached.
    #[inline(always)]
    pub fn peek_ref(&mut self) -> Option<&'src I::Token>
    where
        I: BorrowInput,
    {
        // SAFETY: cursor was generated by previous call to `Input::next`
        unsafe { I::next_ref(self.cache, &mut self.cursor.clone()) }
    }

    /// Skip the next token in the input.
    #[inline(always)]
    pub fn skip(&mut self)
    where
        I: ValueInput,
    {
        let _ = self.next_inner();
    }

    #[cfg_attr(not(feature = "regex"), allow(dead_code))]
    #[inline]
    pub(crate) fn full_slice(&mut self) -> SliceOf<'src, I>
    where
        I: SliceInput,
    {
        I::full_slice(self.cache)
    }

    /// Get a slice of the input that covers the given cursor range.
    #[inline]
    pub fn slice(&mut self, range: Range<&Cursor<'src, 'parse, I>>) -> SliceOf<'src, I>
    where
        I: SliceInput,
    {
        // SAFETY: cursor was generated by previous call to `Input::next`
        unsafe { I::slice(self.cache, &range.start.inner..&range.end.inner) }
    }

    /// Get a slice of the input that covers the given cursor range.
    #[inline]
    pub fn slice_from(&mut self, range: RangeFrom<&Cursor<'src, 'parse, I>>) -> SliceOf<'src, I>
    where
        I: SliceInput,
    {
        // SAFETY: cursor was generated by previous call to `Input::next`
        unsafe { I::slice_from(self.cache, &range.start.inner..) }
    }

    /// Get a slice of the input that covers the given cursor range.
    #[inline]
    pub fn slice_since(&mut self, range: RangeFrom<&Cursor<'src, 'parse, I>>) -> SliceOf<'src, I>
    where
        I: SliceInput,
    {
        // SAFETY: cursor was generated by previous call to `Input::next`
        unsafe { I::slice(self.cache, &range.start.inner..&self.cursor) }
    }

    #[cfg_attr(not(feature = "lexical-numbers"), allow(dead_code))]
    #[inline(always)]
    pub(crate) fn slice_trailing_inner(&mut self) -> SliceOf<'src, I>
    where
        I: SliceInput,
    {
        // SAFETY: cursor was generated by previous call to `Input::next`
        unsafe { I::slice_from(self.cache, &self.cursor..) }
    }

    // /// Get a span over the input that covers the given cursor range.
    // #[inline(always)]
    // pub fn span<'src>(&self, range: Range<&Cursor<'src, 'parse, I>>) -> SpanOf<'src,I> {
    //     // SAFETY: `Cursor` is invariant over 'parse, so we know that this cursor came from the same input
    //     // See `https://plv.mpi-sws.org/rustbelt/ghostcell/`
    //     unsafe { SpanOf<'src,I>(self.cache, &range.start.inner..&range.end.inner) }
    // }

    /// Get a span over the input that goes from the given cursor to the end of the input.
    // TODO: Unify with `InputRef::span`
    #[inline(always)]
    pub fn span_from(&mut self, range: RangeFrom<&Cursor<'src, 'parse, I>>) -> I::Span
    where
        I: ExactSizeInput,
    {
        // SAFETY: `Cursor` is invariant over 'parse, so we know that this cursor came from the same input
        // See `https://plv.mpi-sws.org/rustbelt/ghostcell/`
        unsafe { I::span_from(self.cache, &range.start.inner..) }
    }

    /// Generate a span that extends from the provided [`Cursor`] to the current input position.
    #[inline(always)]
    pub fn span_since(&mut self, before: &Cursor<'src, 'parse, I>) -> I::Span {
        // SAFETY: `Cursor` is invariant over 'parse, so we know that this cursor came from the same input
        // See `https://plv.mpi-sws.org/rustbelt/ghostcell/`
        unsafe { I::span(self.cache, &before.inner..&self.cursor) }
    }

    /// SAFETY: Previous cursor + skip must not exceed length
    #[inline(always)]
    #[cfg(any(feature = "regex", feature = "lexical-numbers"))]
    pub(crate) unsafe fn skip_bytes(&mut self, skip: usize)
    where
        I: SliceInput<Cursor = usize>,
    {
        self.cursor += skip;
    }

    /// Emits a non-fatal error at the current position. This is equivalent to
    /// `emit_at(self.cursor(), error)`.
    #[inline]
    pub fn emit(&mut self, error: ErrOfEx<'src, I, E>) {
        self.emit_inner(None, error);
    }

    /// Emits a non-fatal error at the given cursor position.
    #[inline]
    pub fn emit_at(&mut self, cursor: Cursor<'src, 'parse, I>, error: ErrOfEx<'src, I, E>) {
        self.emit_inner(Some(cursor), error);
    }

    #[inline]
    fn emit_inner(
        &mut self,
        cursor: impl Into<Option<Cursor<'src, 'parse, I>>>,
        error: ErrOfEx<'src, I, E>,
    ) {
        let cursor = cursor
            .into()
            .map(|c| c.inner)
            .unwrap_or_else(|| self.cursor.clone());
        self.errors.secondary.push(Located::at(cursor, error));
    }
    #[inline]
    pub(crate) fn fail_with<D: Driver, O: Hkt, Exp, L>(
        &mut self,
        mk: impl FnOnce(&mut Self) -> (Exp, Option<MaybeRef<'src, I::Token>>, I::Span),
    ) -> PResult<D::Mode, O::Of<'src>>
    where
        Exp: IntoIterator<Item = L>,
        ErrOfEx<'src, I, E>: LabelError<'src, I::Token, I::Span, L>,
    {
        if D::Policy::STRICT {
            return Err(());
        }
        let (expected, found, span) = mk(self);
        self.add_alt_impl(expected, found, span);
        Err(())
    }

    #[inline]
    pub(crate) fn fail_rewind_with<D: Driver, Exp, O: Hkt,L>(
        &mut self,
        checkpoint: input::Checkpoint<'src, 'parse, I, <E::State as Inspector<I>>::Checkpoint>,
        mk: impl FnOnce(&mut Self, &I::Span) -> (Exp, Option<MaybeRef<'src, I::Token>>),
    ) -> PResult<D::Mode, O::Of<'src>>
    where
        Exp: IntoIterator<Item = L>,
        ErrOfEx<'src, I, E>: LabelError<'src, I::Token, I::Span, L>,
    {
        if D::Policy::STRICT {
            self.rewind(checkpoint);
            return Err(());
        }

        let span = self.span_since(checkpoint.cursor());

        let (expected, found) = mk(self, &span);

        self.rewind(checkpoint);

        self.add_alt_impl(expected, found, span);

        Err(())
    }
    #[inline]
    pub(crate) fn add_alt_with<D: Driver, Exp, L>(
        &mut self,
        mk: impl FnOnce(&mut Self) -> (Exp, Option<MaybeRef<'src, I::Token>>, I::Span),
    ) where
        Exp: IntoIterator<Item = L>,
        ErrOfEx<'src, I, E>: LabelError<'src, I::Token, I::Span, L>,
    {
        if D::Policy::STRICT {
            return;
        }
        let (expected, found, span) = mk(self);
        self.add_alt_impl(expected, found, span);
    }
    #[inline]
    pub(crate) fn add_alt_impl<Exp, L>(
        &mut self,
        expected: Exp,
        found: Option<MaybeRef<'src, I::Token>>,
        span: I::Span,
    ) where
        Exp: IntoIterator<Item = L>,
        ErrOfEx<'src, I, E>: LabelError<'src, I::Token, I::Span, L>,
    {
        // Fast path: if the error doesn't carry meaningful information, avoid unnecessary decision-making!
        if core::mem::size_of::<ErrOfEx<'src, I, E>>() == 0 {
            self.errors.alt = Some(Located::at(
                self.cursor.clone(),
                LabelError::expected_found(expected, found, span),
            ));
            return;
        }

        let at = &self.cursor;

        // Prioritize errors before choosing whether to generate the alt (avoids unnecessary error creation)
        self.errors.alt = Some(match self.errors.alt.take() {
            Some(alt) => match I::cursor_location(&alt.pos).cmp(&I::cursor_location(at)) {
                Ordering::Equal => {
                    Located::at(alt.pos, alt.err.merge_expected_found(expected, found, span))
                }
                Ordering::Greater => alt,
                Ordering::Less => Located::at(
                    at.clone(),
                    alt.err.replace_expected_found(expected, found, span),
                ),
            },
            None => Located::at(
                at.clone(),
                LabelError::expected_found(expected, found, span),
            ),
        });
    }
    #[inline]
    pub(crate) fn add_alt_err_with<D: Driver>(
        &mut self,
        at: &CursorOf<'src, I>,
        mk: impl FnOnce() -> ErrOfEx<'src, I, E>,
    ) {
        if D::Policy::STRICT {
            return;
        }
        let err = mk();
        self.add_alt_err_impl::<D>(at, err); // rename your existing body similarly
    }

    #[inline]
    pub(crate) fn add_alt_err_impl<D: Driver>(
        &mut self,
        at: &CursorOf<'src, I>,
        err: ErrOfEx<'src, I, E>,
    ) {
        if D::Policy::STRICT {
            return;
        }
        // Fast path: if the error doesn't carry meaningful information, avoid unnecessary decision-making!
        if core::mem::size_of::<ErrOfEx<'src, I, E>>() == 0 {
            self.errors.alt = Some(Located::at(self.cursor.clone(), err));
            return;
        }

        // Prioritize errors
        self.errors.alt = Some(match self.errors.alt.take() {
            Some(alt) => match I::cursor_location(&alt.pos).cmp(&I::cursor_location(at)) {
                Ordering::Equal => Located::at(alt.pos, alt.err.merge(err)),
                Ordering::Greater => alt,
                Ordering::Less => Located::at(at.clone(), err),
            },
            None => Located::at(at.clone(), err),
        });
    }

    // Take the alt error, if one exists
    pub(crate) fn take_alt(&mut self) -> Option<Located<CursorOf<'src, I>, ErrOfEx<'src, I, E>>> {
        self.errors.alt.take()
    }
}

/// Struct used in [`Parser::validate`] to collect user-emitted errors
pub struct Emitter<E> {
    emitted: Vec<E>,
}

impl<E> Emitter<E> {
    #[inline]
    pub(crate) fn new() -> Emitter<E> {
        Emitter {
            emitted: Vec::new(),
        }
    }

    #[inline]
    pub(crate) fn errors(self) -> Vec<E> {
        self.emitted
    }

    /// Emit a non-fatal error
    #[inline]
    pub fn emit(&mut self, err: E) {
        self.emitted.push(err)
    }
}

/// See [`Parser::map_with`].
pub struct MapExtra<'src, 'b, I: Input, E: ParserExtra<I>> {
    before: &'b CursorOf<'src, I>,
    after: &'b CursorOf<'src, I>,
    cache: &'b mut CacheOf<'src, I>,
    state: &'b mut E::State,
    ctx: &'b CtxOf<'src, I, E>,
    emitted: &'b mut Errors<CursorOf<'src, I>, ErrOfEx<'src, I, E>>,
}

impl<'src, 'b, I: Input, E: ParserExtra<I>> MapExtra<'src, 'b, I, E> {
    #[inline(always)]
    pub(crate) fn new<'parse>(
        before: &'b Cursor<'src, 'parse, I>,
        inp: &'b mut InputRef<'src, 'parse, I, E>,
    ) -> Self {
        Self {
            before: &before.inner,
            after: &inp.cursor,
            cache: inp.cache,
            ctx: inp.ctx,
            state: inp.state,
            emitted: &mut inp.errors,
        }
    }

    /// Get the span corresponding to the output.
    #[inline(always)]
    pub fn span(&mut self) -> I::Span {
        // SAFETY: The cursors both came from the same input
        // TODO: Should this make `MapExtra::new` unsafe? Probably, but it's an internal API and we simply wouldn't
        // ever abuse it in this way, even accidentally.
        unsafe { I::span(self.cache, self.before..self.after) }
    }

    /// Get the slice corresponding to the output.
    #[inline(always)]
    pub fn slice(&mut self) -> SliceOf<'src, I>
    where
        I: SliceInput,
    {
        // SAFETY: The cursors both came from the same input
        unsafe { I::slice(self.cache, self.before..self.after) }
    }

    /// Get the parser state.
    #[inline(always)]
    pub fn state(&mut self) -> &mut E::State {
        self.state
    }

    /// Get the current parser context.
    #[inline(always)]
    pub fn ctx(&self) -> &CtxOf<'src, I, E> {
        self.ctx
    }

    /// Emits an non-fatal error.
    pub fn emit(&mut self, err: ErrOfEx<'src, I, E>) {
        self.emitted
            .secondary
            .push(Located::at(self.before.clone(), err));
    }
}
