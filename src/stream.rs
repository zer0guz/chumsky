use crate::input::{CacheOf, CursorOf, HandleOf, InputFor, MaybeTokenOf};

use super::*;

/// An input that dynamically pulls tokens from a cached [`Iterator`].
///
/// Internally, the stream will pull tokens in batches and cache the results on the heap so as to avoid invoking the
/// iterator every time a new token is required.
///
/// Note: This input type should be used when the internal iterator type, `I`, is *expensive* to clone. This is usually
/// not the case: you might find that [`IterInput`] performs better.
pub struct Stream<I: Iterator> {
    tokens: Vec<I::Item>,
    iter: I,
}

impl<I: Iterator> Stream<I> {
    /// Create a new stream from an [`Iterator`].
    ///
    /// # Example
    ///
    /// ```
    /// # use chumsky::{prelude::*, input::Stream};
    /// let stream = Stream::from_iter((0..10).map(|i| char::from_digit(i, 10).unwrap()));
    ///
    /// let parser = any::<_, extra::Err<Simple<_>>>().filter(|c: &char| c.is_ascii_digit()).repeated().collect::<String>();
    ///
    /// assert_eq!(parser.parse(stream).into_result().as_deref(), Ok("0123456789"));
    /// ```
    pub fn from_iter<J: IntoIterator<IntoIter = I>>(iter: J) -> Self {
        Self {
            tokens: Vec::new(),
            iter: iter.into_iter(),
        }
    }

    /// Box this stream, turning it into a [BoxedStream]. This can be useful in cases where your parser accepts input
    /// from several different sources and it needs to work with all of them.
    pub fn boxed<'a>(self) -> BoxedStream<'a, I::Item>
    where
        I: 'a,
    {
        Stream {
            tokens: self.tokens,
            iter: Box::new(self.iter),
        }
    }

    /// Like [`Stream::boxed`], but yields an [`BoxedExactSizeStream`], which implements [`ExactSizeInput`].
    pub fn exact_size_boxed<'a>(self) -> BoxedExactSizeStream<'a, I::Item>
    where
        I: ExactSizeIterator + 'a,
    {
        Stream {
            tokens: self.tokens,
            iter: Box::new(self.iter),
        }
    }
}

/// A stream containing a boxed iterator. See [`Stream::boxed`].
pub type BoxedStream<'a, T> = Stream<Box<dyn Iterator<Item = T> + 'a>>;

/// A stream containing a boxed exact-sized iterator. See [`Stream::exact_size_boxed`].
pub type BoxedExactSizeStream<'a, T> = Stream<Box<dyn ExactSizeIterator<Item = T> + 'a>>;

impl<I: Iterator> Sealed for Stream<I> {}

impl<'src, I: Iterator> InputFor<'src, I::Item> for Stream<I> {
    type MaybeToken = I::Item;

    type Cursor = usize;

    type Cache = Self;
    
    type Handle = Self;
}

impl<I: Iterator> Input for Stream<I>
where
    I::Item: Clone,
{
    type Span = SimpleSpan<usize>;

    type Token = I::Item;
    #[inline(always)]
    fn begin<'src>(me: CacheOf<'src,Self>) -> (CursorOf<'src, Self>, CacheOf<'src, Self>) {
        (0, me)
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
        unsafe { Self::next(this, cursor) }
    }

    #[inline(always)]
    unsafe fn span<'src>(
        _this: &mut CacheOf<'src, Self>,
        range: Range<&CursorOf<'src, Self>>,
    ) -> Self::Span {
        (*range.start..*range.end).into()
    }
}

impl<I: ExactSizeIterator> ExactSizeInput for Stream<I>
where
    I::Item: Clone,
{
    #[inline(always)]
    unsafe fn span_from<'src>(
        this: &mut CacheOf<'src, Self>,
        range: RangeFrom<&CursorOf<'src, Self>>,
    ) -> Self::Span {
        (*range.start..this.tokens.len() + this.iter.len()).into()
    }
}

impl<I: Iterator> ValueInput for Stream<I>
where
    I::Item: Clone,
{
    #[inline]
    unsafe fn next<'src>(
        this: &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<I::Item> {
        // Pull new items into the vector if we need them
        if this.tokens.len() <= *cursor {
            this.tokens.extend((&mut this.iter).take(512));
        }

        // Get the token at the given cursor
        this.tokens.get(*cursor).map(|tok| {
            *cursor += 1;
            tok.clone()
        })
    }
}

/// An input that dynamically pulls tokens from an [`Iterator`].
///
/// This input type supports rewinding by [`Clone`]-ing the iterator. It is recommended that your iterator is very
/// cheap to clone. If this is not the case, consider using [`Stream`] instead, which caches generated tokens
/// internally.
pub struct IterInput<I, S, T> {
    iter: I,
    eoi: S,
    _t: EmptyPhantom<T>,
}

impl<'src,I, S, T> IterInput<I, S, T> {
    /// Create a new [`IterInput`] with the given iterator, and end of input span.
    pub fn new(iter: I, eoi: S) -> Self {
        Self {
            iter,
            eoi,
            _t: EmptyPhantom::new(),
        }
    }
}

impl<'src, I, S: Span, T> InputFor<'src, T> for IterInput<I, S, T>
where
    I: Iterator<Item = (T, S)> + Clone,
{
    type Cursor = (I, usize, Option<S::Offset>);

    type MaybeToken = T;

    type Cache = S; //eoi
    
    type Handle = Self; 
}

impl<I, T, S> Input for IterInput<I, S, T>
where
    S: Span,
    I: Iterator<Item = (T, S)> + Clone,
{
    type Span = S;

    type Token = T;
    #[inline]
    fn begin<'src>(me: HandleOf<'src,Self>) -> (CursorOf<'src, Self>, CacheOf<'src, Self>) {
        ((me.iter.clone(), 0, None), me.eoi)
    }

    #[inline]
    fn cursor_location<'src>(cursor: &CursorOf<'src, Self>) -> usize {
        cursor.1
    }

    unsafe fn next_maybe<'src>(
        _eoi: &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<MaybeTokenOf<'src, Self>> {
        cursor.0.next().map(|(tok, span)| {
            cursor.1 += 1;
            cursor.2 = Some(span.end());
            tok
        })
    }

    unsafe fn span<'src>(eoi: &mut CacheOf<'src, Self>, range: Range<&CursorOf<'src, Self>>) -> S {
        match range.start.0.clone().next() {
            Some((_, s)) => {
                let end = range.end.2.clone().unwrap_or_else(|| eoi.end());
                S::new(eoi.context(), s.start()..end)
            }
            None => S::new(eoi.context(), eoi.end()..eoi.end()),
        }
    }
}

// impl<'src, I, S> ExactSizeInput for IterInput<I, S>
// where
//     I: Iterator<Item = (T, S)> + Clone + 'src,
//     S: Span + 'src,
// {
//     #[inline(always)]
//     unsafe fn span_from(this: &mut CacheOf<'src,Self>, range: RangeFrom<&CursorOf<'src,Self>>) -> SpanOf<'src,Self> {
//         (*range.start..this.tokens.len() + cursor.0.len()).into()
//     }
// }

impl<I, T, S> ValueInput for IterInput<I, S, T>
where
    I: Iterator<Item = (T, S)> + Clone,
    S: Span,
{
    #[inline]
    unsafe fn next<'src>(
        this: &mut CacheOf<'src, Self>,
        cursor: &mut CursorOf<'src, Self>,
    ) -> Option<T> {
        unsafe { Self::next_maybe(this, cursor) }
    }
}

// #[test]
// fn map_tuple() {
//     fn parser<'src, I: Input<Token = char>>() -> impl Parser<I, char> {
//         just('h')
//     }

//     let stream: Stream<Box<dyn Iterator<Item = (char, Range<i32>)>>> =
//         Stream::from_iter(core::iter::once(('h', 0..1))).boxed();
//     let stream = stream.split_token_span(0..10);

//     assert_eq!(parser().parse(stream).into_result(), Ok('h'));
// }
