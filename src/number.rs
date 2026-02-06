//! TODO: Add documentation when approved

use crate::extra::ErrOfEx;
use crate::input::SliceOf;

use super::*;
pub use lexical::format;

use lexical::FromLexical;
use lexical::parse_partial;

/// TODO: Add documentation when approved
pub struct Number<const F: u128, I: ?Sized, O, E> {
    #[allow(dead_code)]
    phantom: EmptyPhantom<(*const I, E, O)>,
}

impl<const F: u128, I, O, E> Copy for Number<F, I, O, E> {}
impl<const F: u128, I, O, E> Clone for Number<F, I, O, E> {
    fn clone(&self) -> Self {
        *self
    }
}

/// TODO: Add documentation when approved
pub const fn number<const F: u128, I: ?Sized, O, E>() -> Number<F, I, O, E> {
    Number::<F, I, O, E> {
        phantom: EmptyPhantom::new(),
    }
}

/// A label denoting a parseable number.
pub struct ExpectedNumber;

impl<const F: u128, I, O, E> Parser<I, O, E> for Number<F, I, O, E>
where
    I: SliceInput<Cursor = usize> + ?Sized,
    for<'src> SliceOf<'src,I>: AsRef<[u8]>,
    E: ParserExtra<I>,
    for<'src> ErrOfEx<'src,I,E>: LabelError<'src, I::Token,I::Span, ExpectedNumber>,
    O: Hkt,
    for<'src> O::Of<'src>: FromLexical,
{
    #[inline]
    fn go<'src, D: Driver>(
        &self,
        inp: &mut InputRef<'src, '_, I, E>,
    ) -> PResult<D::Mode, O::Of<'src>> {
        let before = inp.cursor();
        match parse_partial(inp.slice_trailing_inner().as_ref()) {
            Ok((out, skip)) => {
                // SAFETY: `skip` is no longer than the trailing input's byte length
                unsafe { inp.skip_bytes(skip) };
                Ok(D::Mode::bind(|| out))
            }
            Err(_err) => {
                // TODO: Improve error
                inp.add_alt_with::<D, _, _>(|inp| {
                    ([ExpectedNumber], None, inp.span_since(&before))
                });
                Err(())
            }
        }
    }

    go_extra!(O);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Parser, extra};
    use lexical::format::RUST_LITERAL;

    // These have been shamelessly yanked from the rust test-float-parse suite.
    // More specifically:
    //
    // https://github.com/rust-lang/rust/tree/64185f205dcbd8db255ad6674e43c63423f2369a/src/etc/test-float-parse
    mod rust {
        use super::*;

        const FLOAT: Number<RUST_LITERAL, str, Id<f64>, extra::Default> = number();

        fn validate(test: &mut str) {
            FLOAT.parse(test).unwrap();
        }

        #[test]
        fn few_ones() {
            let mut pow = vec![];
            for i in 0..63 {
                pow.push(1u64 << i);
            }
            for a in &pow {
                for b in &pow {
                    for c in &pow {
                        validate(&mut (a | b | c).to_string());
                    }
                }
            }
        }

        #[test]
        fn huge_pow10() {
            for e in 300..310 {
                for i in 0..100000 {
                    validate(&mut format!("{i}e{e}"));
                }
            }
        }

        #[test]
        fn long_fraction() {
            for n in 0..10 {
                let digit = char::from_digit(n, 10).unwrap();
                let mut s = "0.".to_string();
                for _ in 0..400 {
                    s.push(digit);
                    if s.parse::<f64>().is_ok() {
                        validate(&mut s);
                    }
                }
            }
        }

        #[test]
        fn short_decimals() {
            for e in 1..301 {
                for i in 0..10000 {
                    if i % 10 == 0 {
                        continue;
                    }

                    validate(&mut format!("{i}e{e}"));
                    validate(&mut format!("{i}e-{e}"));
                }
            }
        }

        #[test]
        fn subnorm() {
            for bits in 0u32..(1 << 21) {
                let single: f32 = f32::from_bits(bits);
                validate(&mut format!("{single:e}"));
                let double: f64 = f64::from_bits(bits as u64);
                validate(&mut format!("{double:e}"));
            }
        }

        #[test]
        fn tiny_pow10() {
            for e in 301..327 {
                for i in 0..100000 {
                    validate(&mut format!("{i}e-{e}"));
                }
            }
        }

        #[test]
        fn u32_small() {
            for i in 0..(1 << 19) {
                validate(&mut i.to_string());
            }
        }

        #[test]
        fn u64_pow2() {
            for exp in 19..64 {
                let power: u64 = 1 << exp;
                validate(&mut power.to_string());
                for offset in 1..123 {
                    validate(&mut (power + offset).to_string());
                    validate(&mut (power - offset).to_string());
                }
            }
            for offset in 0..123 {
                validate(&mut (u64::MAX - offset).to_string());
            }
        }
    }
}
