//! Types and functions that relate to error recovery.
//!
//! When chumsky encounters an erroneous input that it cannot parse, it can be told to attempt to recover from the
//! error using a variety of strategies (you can also create your own strategies).
//!
//! There is no silver bullet strategy for error recovery. By definition, if the input to a parser is invalid then the
//! parser can only make educated guesses as to the meaning of the input. Different recovery strategies will work
//! better for different languages, and for different patterns within those languages.
//!
//! Chumsky provides a variety of recovery strategies (each implementing the `Strategy` trait), but it's important to
//! understand that all of
//!
//! - which you apply
//! - where you apply them
//! - what order you apply them
//!
//! will greatly affect the quality of the errors that Chumsky is able to produce, along with the extent to which it
//! is able to recover a useful AST. Where possible, you should attempt more 'specific' recovery strategies first
//! rather than those that mindlessly skip large swathes of the input.
//!
//! It is recommended that you experiment with applying different strategies in different situations and at different
//! levels of the parser to find a configuration that you are happy with. If none of the provided error recovery
//! strategies cover the specific pattern you wish to catch, you can even create your own by digging into Chumsky's
//! internals and implementing your own strategies! If you come up with a useful strategy, feel free to open a PR
//! against the [main repository](https://github.com/zesterer/chumsky/)!

use crate::hkt::Lt;

use super::*;

/// A trait implemented by error recovery strategies. See [`Parser::recover_with`].
///
/// This trait is sealed and so cannot be implemented by other crates because it has an unstable API. This may
/// eventually change. For now, if you wish to implement a new strategy, consider using [`via_parser`] or
/// [opening an issue/PR](https://github.com/zesterer/chumsky/issues/new).
pub trait Strategy<I: Input, O: Hkt, E: ParserExtra<I> = extra::Default>: Sealed {
    // Attempt to recover from a parsing failure.
    // The strategy should properly handle the alt error but is not required to handle rewinding.
    #[doc(hidden)]
    fn recover<'src, D: Driver, P: Parser<I, O, E>>(
        &self,
        inp: &mut InputRef<'src, '_, I, E>,
        parser: &P,
    ) -> PResult<D::Mode, O::Of<'src>>;
}

/// See [`via_parser`].
#[derive(Copy, Clone)]
pub struct ViaParser<A>(A);

/// Recover via the given recovery parser.
pub fn via_parser<A>(parser: A) -> ViaParser<A> {
    ViaParser(parser)
}

impl<A> Sealed for ViaParser<A> {}
impl<I, O, E, A> Strategy<I, O, E> for ViaParser<A>
where
    I: Input,
    A: Parser<I, O, E>,
    E: ParserExtra<I>,
    O: Hkt,
{
    fn recover<'src, D: Driver, P: Parser<I, O, E>>(
        &self,
        inp: &mut InputRef<'src, '_, I, E>,
        _parser: &P,
    ) -> PResult<D::Mode, O::Of<'src>> {
        let alt = inp.take_alt().unwrap(); // Can't fail!
        let out = match self.0.go::<D>(inp) {
            Ok(out) => out,
            Err(()) => {
                inp.errors.alt = Some(alt);
                return Err(());
            }
        };
        inp.emit(alt.err);
        Ok(out)
    }
}

/// See [`Parser::recover_with`].
#[derive(Copy, Clone)]
pub struct RecoverWith<A, S> {
    pub(crate) parser: A,
    pub(crate) strategy: S,
}

impl<I, O, E, A, S> Parser<I, O, E> for RecoverWith<A, S>
where
    I: Input,
    E: ParserExtra<I>,
    A: Parser<I, O, E>,
    S: Strategy<I, O, E>,
    O: Hkt,
{
    #[doc(hidden)]
    #[cfg(feature = "debug")]
    fn node_info(&self, scope: &mut debug::NodeScope) -> debug::NodeInfo {
        self.parser.node_info(scope)
    }

    fn go<'src, D: Driver>(
        &self,
        inp: &mut InputRef<'src, '_, I, E>,
    ) -> PResult<D::Mode, O::Of<'src>> {
        let before = inp.save();
        match self.parser.go::<D>(inp) {
            Ok(out) => Ok(out),
            Err(()) => {
                if <D::Policy as Policy>::STRICT {
                    Err(())
                } else {
                    inp.rewind(before.clone());
                    match self.strategy.recover::<D, _>(inp, &self.parser) {
                        Ok(out) => Ok(out),
                        Err(()) => {
                            // Reset to before fallback attempt
                            inp.rewind(before);
                            Err(())
                        }
                    }
                }
            }
        }
    }

    go_extra!(O);
}

/// See [`skip_then_retry_until`].
#[must_use]
#[derive(Copy, Clone)]
pub struct SkipThenRetryUntil<S, U> {
    skip: S,
    until: U,
}

impl<S, U> Sealed for SkipThenRetryUntil<S, U> {}
impl<I, O, E, S, U> Strategy<I, O, E> for SkipThenRetryUntil<S, U>
where
    I: Input,
    S: Parser<I, (), E>,
    U: Parser<I, (), E>,
    E: ParserExtra<I>,
    O: Hkt,
{
    fn recover<'src, D: Driver, P: Parser<I, O, E>>(
        &self,
        inp: &mut InputRef<'src, '_, I, E>,
        parser: &P,
    ) -> PResult<D::Mode, O::Of<'src>> {
        let alt = inp.take_alt().unwrap(); // Can't fail!
        loop {
            let before = inp.save();
            if let Ok(()) = self.until.go::<DoCheck<D>>(inp) {
                inp.errors.alt = Some(alt);
                inp.rewind(before);
                break Err(());
            } else {
                inp.rewind(before);
            }

            if let Err(()) = self.skip.go::<DoCheck<D>>(inp) {
                inp.errors.alt = Some(alt);
                break Err(());
            }

            let before = inp.save();
            if let Some(out) = parser.go::<D>(inp).ok().filter(|_| {
                inp.errors
                    .secondary_errors_since(before.err_count)
                    .is_empty()
            }) {
                inp.emit(alt.err);
                break Ok(out);
            } else {
                inp.errors.alt.take();
                inp.rewind(before);
            }
        }
    }
}

/// TODO
pub fn skip_then_retry_until<S, U>(skip: S, until: U) -> SkipThenRetryUntil<S, U> {
    SkipThenRetryUntil { skip, until }
}

/// See [`skip_until`].
#[must_use]
#[derive(Copy, Clone)]
pub struct SkipUntil<S, U, F> {
    skip: S,
    until: U,
    fallback: F,
}

impl<S, U, F> Sealed for SkipUntil<S, U, F> {}
impl<I, O, E, S, U, F> Strategy<I, O, E> for SkipUntil<S, U, F>
where
    I: Input,
    S: Parser<I, (), E>,
    U: Parser<I, (), E>,
    F: for<'src> Fn(Lt<'src>) -> O::Of<'src>,
    E: ParserExtra<I>,
    O: Hkt,
{
    fn recover<'src, D: Driver, P: Parser<I, O, E>>(
        &self,
        inp: &mut InputRef<'src, '_, I, E>,
        _parser: &P,
    ) -> PResult<D::Mode, O::Of<'src>> {
        let alt = inp.take_alt().unwrap(); // Can't fail!
        loop {
            let before = inp.save();
            if let Ok(()) = self.until.go::<DoCheck<D>>(inp) {
                inp.emit(alt.err);
                break Ok(D::Mode::bind(|| (self.fallback)(Lt::new())));
            }
            inp.rewind(before);

            if let Err(()) = self.skip.go::<DoCheck<D>>(inp) {
                inp.errors.alt = Some(alt);
                break Err(());
            }
        }
    }
}

/// A recovery parser that skips input until one of several inputs is found.
///
/// This strategy is very 'stupid' and can result in very poor error generation in some languages. Place this strategy
/// after others as a last resort, and be careful about over-using it.
pub fn skip_until<S, U, F>(skip: S, until: U, fallback: F) -> SkipUntil<S, U, F> {
    SkipUntil {
        skip,
        until,
        fallback,
    }
}

// /// A recovery parser that searches for a start and end delimiter, respecting nesting.
// ///
// /// It is possible to specify additional delimiter pairs that are valid in the pattern's context for better errors. For
// /// example, you might want to also specify `[('[', ']'), ('{', '}')]` when recovering a parenthesized expression as
// /// this can aid in detecting delimiter mismatches.
// ///
// /// A function that generates a fallback output on recovery is also required.
// // TODO: Make this a strategy, add an unclosed_delimiter error
// pub fn nested_delimiters<'parse, I, O, E, F, const N: usize>(
//     start: I::Token,
//     end: I::Token,
//     others: [(I::Token, I::Token); N],
//     fallback: F,
// ) -> impl Parser<I, O, E> + Clone + 'parse
// where
//     I: ValueInput + 'parse,
//     I::Token: PartialEq + Clone,
//     E: extra::ParserExtra<I> + 'parse,
//     F: Fn(I::Span,Lt<'parse>) -> O::Of<'parse> + Clone,
//     O: Hkt,
// {
//     // TODO: Does this actually work? TESTS!
//     #[allow(clippy::tuple_array_conversions)]
//     // Clippy is overly eager to fine pointless non-problems
//     recursive({
//         let (start, end) = (start.clone(), end.clone());
//         |block| {
//             let mut many_block = Parser::boxed(
//                 block
//                     .clone()
//                     .delimited_by(just(start.clone()), just(end.clone())),
//             );
//             for (s, e) in &others {
//                 many_block = Parser::boxed(
//                     many_block.or(block.clone().delimited_by(just(s.clone()), just(e.clone()))),
//                 );
//             }

//             let skip = [start, end]
//                 .into_iter()
//                 .chain(IntoIterator::into_iter(others).flat_map(|(s, e)| [s, e]))
//                 .collect::<Vec<_>>();

//             many_block
//                 .or(any().and_is(none_of(skip)).ignored())
//                 .repeated()
//         }
//     })
//     .delimited_by(just(start), just(end))
//     .map_with(move |_, e| fallback(e.span(),Lt::new()))
// }
