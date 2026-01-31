//! Generic error, state and context types for parsers
//! Useful for custom allocation, error handling, context-specific parsers, and more.

use inspector::Inspector;
pub use inspector::SimpleState;

use crate::error::ErrorHkt;

use super::*;

type DefaultErr = EmptyErr;
type DefaultState = ();
type DefaultCtx = ();

/// A trait for extra types on a [`Parser`] that control the behavior of certain combinators and output.
///
/// Currently, this consists of the error type emitted, the state type used in the `*_state` combinators,
/// and the context type used in the `*_ctx` and `*configure` parsers.
///
/// This trait is sealed and so cannot be implemented by other crates because all uses should instead
/// go through the types defined in this module.
pub trait ParserExtra<I>: Sealed
where
    I: Input,
{
    /// Error type to use for the parser. This type must implement [`Error`], and when it fails,
    /// the parser will return a set of this type to describe why the failure occurred.
    type ErrorFam: ErrorHkt<I::Token,I::Span>; 
    /// State type to use for the parser. This is used to provide stateful *output* of the parser,
    /// such as interned identifiers or position-dependent name resolution, however *cannot* influence
    /// the actual progress of the parser - for that, use [`Self::Context`].
    ///
    /// For examples of using this type, see [`Parser::map_with`] or [`Parser::foldl_with`].
    type State: Inspector<I>;
    /// Context used for parser configuration. This is used to provide context-sensitive parsing of *input*.
    /// Context-sensitive parsing in chumsky is always left-hand sensitive - context for the parse must originate
    /// from an earlier point in the stream than the parser relying on it. This can affect the output of a parser,
    /// but for things that don't wish to alter the actual rules of parsing, one should instead prefer [`Self::State`].
    ///
    /// For examples of using this type, see [`Parser::ignore_with_ctx`], [`Parser::then_with_ctx`] and [`ConfigParser::configure`].
    type ContextFam: Hkt;
}

/// Use all default extra types. See [`ParserExtra`] for more details.
pub type Default = Full<DefaultErr, DefaultState, DefaultCtx>;

/// Use specified error type, but default other types. See [`ParserExtra`] for more details.
pub type Err<E> = Full<E, DefaultState, DefaultCtx>;

/// Use specified state type, but default other types. See [`ParserExtra`] for more details.
///
/// Use `State<S>` or `Full<E, S, C>` as the `Extra` type parameter of a parser to use a custom state type.
/// You can then use `parser().parse_with_state(&mut S,strict)` to parse with a custom state.
///
/// See [`Parser::map_with`] for examples.
pub type State<S> = Full<DefaultErr, S, DefaultCtx>;

/// Use specified context type, but default other types. See [`ParserExtra`] for more details.
pub type Context<C> = Full<DefaultErr, DefaultState, C>;

/// Specify all extra types. See [`ParserExtra`] for more details.
pub struct Full<E, S, C>(PhantomData<(E, S, C)>);

impl<E, S, C> Sealed for Full<E, S, C> {}
impl<I, E, S, C> ParserExtra<I> for Full<E, S, C>
where
    I: Input,
    S: Inspector<I>,
    C: Hkt, 
    E: error::ErrorHkt<I::Token,I::Span>
{
    type ErrorFam  = E;
    type State = S;
    type ContextFam = C;
}

pub type CtxOf<'src, I, Ex> = <<Ex as ParserExtra<I>>::ContextFam as Hkt>::Of<'src>;
pub type ErrOfEx<'src, I, Ex> = <<Ex as ParserExtra<I>>::ErrorFam as ErrorHkt<<I as Input>::Token,<I as Input>::Span>>::Err<'src>;


pub struct CtxOut<O>(core::marker::PhantomData<O>);

impl<O: Hkt> Hkt for CtxOut<O> {
    type Of<'src>
        = O::Of<'src>;
}
