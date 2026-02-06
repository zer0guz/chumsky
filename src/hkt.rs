use std::marker::PhantomData;

use crate::{container::{Container, ContainerExactly}, extra::{ErrOfEx, ParserExtra}, input::{Input, MapExtra, SliceInput, SliceOf}, span::WrappingSpan, util::Ref};

pub struct Lt<'src>(PhantomData<&'src()>);

impl<'src> Lt<'src> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}


pub trait Hkt {
    type Of<'src>;
}

impl Hkt for () {
    type Of<'src> = ();
}

pub struct Id<T>(core::marker::PhantomData<T>);
impl<T> Hkt for Id<T> {
    type Of<'src> = T;
}

pub struct RefFam<T>(core::marker::PhantomData<T>);
impl<T> Hkt for RefFam<T> {
    type Of<'src> = Ref<'src,T>;
}

pub struct IterItem<O>(core::marker::PhantomData<O>);

pub struct ResultOut<O, U>(core::marker::PhantomData<(O, U)>);

impl<O: Hkt, U> Hkt for ResultOut<O, U> {
    type Of<'src> = Result<O::Of<'src>, U>;
}

pub struct OptionOut<O>(core::marker::PhantomData<O>);

impl<O: Hkt> Hkt for OptionOut<O> {
    type Of<'src> = Option<O::Of<'src>>;
}

impl<O: Hkt> Hkt for IterItem<O>
where
    for<'src> O::Of<'src>: IntoIterator,
{
    type Of<'src> = <O::Of<'src> as IntoIterator>::Item;
}

pub struct CollectOut<C, O>(core::marker::PhantomData<(C, O)>);

impl<C, O> Hkt for CollectOut<C, O>
where
    C: Container,
    O: Hkt,
{
    type Of<'src> = C::With<O::Of<'src>>;
}



pub struct SliceOut<I>(core::marker::PhantomData<I>);

impl<I: SliceInput> Hkt for SliceOut<I> {
    type Of<'src> = SliceOf<'src,I>;
}

pub struct SpannedOut<I, OA>(core::marker::PhantomData<(I, OA)>);

impl<I, OA> Hkt for SpannedOut<I, OA>
where
    I: Input,
    OA: Hkt,
    for<'src> I::Span: WrappingSpan<OA::Of<'src>>,
{
    type Of<'src> = <I::Span as WrappingSpan<OA::Of<'src>>>::Spanned;
}

pub struct CollectExactlyOut<C, O>(core::marker::PhantomData<(C, O)>);

impl<C, O> Hkt for CollectExactlyOut<C, O>
where
    C: ContainerExactly,
    O: Hkt,
{
    type Of<'src> = C::With<O::Of<'src>>;
}

pub struct VecOut<O>(core::marker::PhantomData<O>);

impl<O: Hkt> Hkt for VecOut<O> {
    type Of<'src> = Vec<O::Of<'src>>;
}


impl<O: Hkt, const N: usize> Hkt for [O; N] {
    type Of<'src> = [O::Of<'src>; N];
}


macro_rules! impl_outfam_for_tuples {
    () => {};

    ($head:ident $($tail:ident)*) => {
        impl_outfam_for_tuples!($($tail)*);
        impl_outfam_for_tuples!(~ $head $($tail)*);
    };

    (~ $($T:ident)*) => {
        impl<$($T: Hkt),*> Hkt for ($($T,)*) {
            type Of<'src> = ($($T::Of<'src>,)*);
        }
    };
}

impl_outfam_for_tuples! {
    A B C D E F G H I J K L M N O P Q R S T U V W X Y Z
}

pub trait MapWithFn<OA: Hkt, I: Input, E: ParserExtra<I>> {
    type Out: Hkt;

    fn apply<'src>(
        &self,
        x: OA::Of<'src>,
        extra: &mut MapExtra<'src, '_, I, E>,
    ) -> <Self::Out as Hkt>::Of<'src>;
}

pub trait MapFn<OA: Hkt, I: Input, E: ParserExtra<I>> {
    type Out: Hkt;

    fn apply<'src>(
        &self,
        x: OA::Of<'src>,
        lt: Lt<'src>,
    ) -> <Self::Out as Hkt>::Of<'src>;
}

pub struct Mapper<F, U>(pub F, core::marker::PhantomData<fn() -> U>);

impl<F:Clone, U> Clone for Mapper<F, U> {
    fn clone(&self) -> Self {
        Self(self.0.clone(), self.1.clone())
    }
}

impl<F, U> Mapper<F, U> {
    pub fn new(f: F) -> Self {
        Self(f, PhantomData)
    }
}

impl<OA, I, E, F, U> MapWithFn<OA, I, E> for Mapper<F, U>
where
    OA: Hkt,
    U: Hkt,
    I: Input,
    E: ParserExtra<I>,
    F: for<'src> Fn(OA::Of<'src>, &mut MapExtra<'src, '_, I, E>) -> U::Of<'src>,
{
    type Out = U;

    #[inline(always)]
    fn apply<'src>(
        &self,
        x: OA::Of<'src>,
        extra: &mut MapExtra<'src, '_, I, E>,
    ) -> U::Of<'src> {
        (self.0)(x, extra)
    }
}

impl<OA, I, E, F, U> MapFn<OA, I, E> for Mapper<F, U>
where
    OA: Hkt,
    U: Hkt,
    I: Input,
    E: ParserExtra<I>,
    F: for<'src> Fn(OA::Of<'src>, Lt<'src>) -> U::Of<'src>,
{
    type Out = U;

    #[inline(always)]
    fn apply<'src>(
        &self,
        x: OA::Of<'src>,
        lt: Lt<'src>,
    ) -> U::Of<'src> {
        (self.0)(x, lt)
    }
}


pub trait TryMapWithFn<OA: Hkt, I: Input, E: ParserExtra<I>> {
    type Out: Hkt;

    fn apply<'src>(
        &self,
        x: OA::Of<'src>,
        extra: &mut MapExtra<'src, '_, I, E>,
    ) -> Result<<Self::Out as Hkt>::Of<'src>, ErrOfEx<'src, I, E>>;
}

impl<OA, I, E, F, U> TryMapWithFn<OA, I, E> for Mapper<F, U>
where
    OA: Hkt,
    I: Input,
    E: ParserExtra<I>,
    U: Hkt,
    F: for<'src> Fn(OA::Of<'src>, &mut MapExtra<'src, '_, I, E>)
        -> Result<U::Of<'src>, ErrOfEx<'src, I, E>>,
{
    type Out = U;

    fn apply<'src>(
        &self,
        x: OA::Of<'src>,
        extra: &mut MapExtra<'src, '_, I, E>,
    ) -> Result<U::Of<'src>, ErrOfEx<'src, I, E>> {
        (self.0)(x, extra)
    }
}


impl Hkt for &str {
    type Of<'src> = &'src str;
}
