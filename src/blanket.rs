use super::*;

impl<T, I, O, E> Parser<I, O, E> for &T
where
    T: ?Sized + Parser<I, O, E>,
    I: Input,
    E: ParserExtra<I>,
    O:Hkt,
{
    #[doc(hidden)]
    #[cfg(feature = "debug")]
    fn node_info(&self, scope: &mut debug::NodeScope) -> debug::NodeInfo {
        (*self).node_info(scope)
    }

    fn go<'src,D: Driver>(&self, inp: &mut InputRef<'src, '_, I, E>) -> PResult<D::Mode, O::Of<'src>>
    where
        Self: Sized,
    {
        D::invoke(*self, inp)
    }

    go_extra!(O);
}

impl<T, I, O, E> ConfigParser<I, O, E> for &T
where
    T: ?Sized + ConfigParser<I, O, E>,
    I: Input,
    E: ParserExtra<I>,
        O:Hkt,

{
    type Config = T::Config;

    fn go_cfg<'src,D: Driver>(
        &self,
        inp: &mut InputRef<'src, '_, I, E>,
        cfg: Self::Config,
    ) -> PResult<D::Mode, O::Of<'src>> {
        D::invoke_cfg(*self, inp, cfg)
    }
}
