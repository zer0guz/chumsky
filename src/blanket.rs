use super::*;

impl<'src, T, I, O, E> Parser<'src, I, O, E> for &T
where
    T: ?Sized + Parser<'src, I, O, E>,
    I: Input + 'src,
    E: ParserExtra<'src, I>,
{
    #[doc(hidden)]
    #[cfg(feature = "debug")]
    fn node_info(&self, scope: &mut debug::NodeScope) -> debug::NodeInfo {
        (*self).node_info(scope)
    }

    fn go<D: Driver>(&self, inp: &mut InputRef<'src, '_, I, E>) -> PResult<D::Mode, O>
    where
        Self: Sized,
    {
        D::invoke(*self, inp)
    }

    go_extra!(O);
}

impl<'src, T, I, O, E> ConfigParser<'src, I, O, E> for &T
where
    T: ?Sized + ConfigParser<'src, I, O, E>,
    I: Input + 'src,
    E: ParserExtra<'src, I>,
{
    type Config = T::Config;

    fn go_cfg<D: Driver>(
        &self,
        inp: &mut InputRef<'src, '_, I, E>,
        cfg: Self::Config,
    ) -> PResult<D::Mode, O> {
        D::invoke_cfg(*self, inp, cfg)
    }
}
