use criterion::{Criterion, black_box, criterion_group, criterion_main};

mod utils;

static CBOR: &[u8] = include_bytes!("samples/sample.cbor");

fn cbor_strict(c: &mut Criterion) {
    c.bench_function("cbor_strict_emptyerr_normal", {
        type Error<'a> = EmptyErr;

        use ::chumsky::prelude::*;
        let cbor = chumsky_zero_copy::cbor::<Error>();
        move |b| {
            b.iter(|| {
                black_box(cbor.parse(black_box(CBOR)))
                    .into_result()
                    .unwrap()
            })
        }
    });
    c.bench_function("cbor_strict_rich_normal", {
        use ::chumsky::prelude::*;
        type Error<'a> = Rich<'a, u8>;

        let cbor = chumsky_zero_copy::cbor::<Error>();
        move |b| {
            b.iter(|| {
                black_box(cbor.parse(black_box(CBOR)))
                    .into_result()
                    .unwrap()
            })
        }
    });
    c.bench_function("cbor_strict_emptyerr_strict", {
        use ::chumsky::prelude::*;
        type Error<'a> = EmptyErr;

        let cbor = chumsky_zero_copy::cbor::<Error>();
        move |b| {
            b.iter(|| {
                black_box(cbor.parse_strict(black_box(CBOR)))
                    .into_result()
                    .unwrap()
            })
        }
    });
    c.bench_function("cbor_strict_rich_strict", {
        use ::chumsky::prelude::*;
        type Error<'a> = Rich<'a, u8>;

        let cbor = chumsky_zero_copy::cbor::<Error>();
        move |b| {
            b.iter(|| {
                black_box(cbor.parse_strict(black_box(CBOR)))
                    .into_result()
                    .unwrap()
            })
        }
    });
}

criterion_group!(
    name = benches;
    config = utils::make_criterion();
    targets = cbor_strict
);

criterion_main!(benches);

#[derive(Debug, Clone, PartialEq)]
pub enum CborZero<'a> {
    Bool(bool),
    Null,
    Undef,
    Int(i64),
    Bytes(&'a [u8]),
    String(&'a str),
    Array(Vec<CborZero<'a>>),
    Map(Vec<(CborZero<'a>, CborZero<'a>)>),
    Tag(u64, Box<CborZero<'a>>),
    // Byte(u8)
    // HalfFloat(f16),
    SingleFloat(f32),
    DoubleFloat(f64),
}

mod chumsky_zero_copy {
    use super::CborZero;
    use chumsky::prelude::*;
    pub fn cbor<'a, Err: chumsky::error::Error<'a, &'a [u8]> + 'a>()
    -> impl Parser<'a, &'a [u8], CborZero<'a>, extra::Err<Err>> {
        recursive(|data| {
            let take = |n: u8| any().map(move |x| x % (1 << n));
            let int = |bytes| {
                empty()
                    .to(0u64)
                    .foldl(any().repeated().exactly(bytes), |a, x| (a << 8) | x as u64)
            };
            let read_uint = choice((
                take(5).filter(|x| *x <= 23).map(|x| x as u64),
                take(5).filter(|x| *x == 24).ignore_then(int(1)),
                take(5).filter(|x| *x == 25).ignore_then(int(2)),
                take(5).filter(|x| *x == 26).ignore_then(int(4)),
                take(5).filter(|x| *x == 27).ignore_then(int(8)),
            ));

            let uint = read_uint.map(|x| CborZero::Int(x.try_into().unwrap()));
            let nint = read_uint.map(|x| CborZero::Int(-1 - i64::try_from(x).unwrap()));

            let length = read_uint.map(|x| usize::try_from(x).unwrap());
            let bstr = length.ignore_with_ctx(
                any()
                    .repeated()
                    .configure(|cfg, ctx| cfg.exactly(*ctx))
                    .to_slice()
                    .map(CborZero::Bytes),
            );

            let str = length.ignore_with_ctx(
                any()
                    .repeated()
                    .configure(|cfg, ctx| cfg.exactly(*ctx))
                    .to_slice()
                    .map(|slice| CborZero::String(std::str::from_utf8(slice).unwrap())),
            );

            let array = length.ignore_with_ctx(
                data.clone()
                    .with_ctx(())
                    .repeated()
                    .configure(|cfg, ctx| cfg.exactly(*ctx))
                    .collect::<Vec<_>>()
                    .map(CborZero::Array),
            );

            let map = length.ignore_with_ctx(
                data.clone()
                    .then(data.clone())
                    .with_ctx(())
                    .repeated()
                    .configure(|cfg, ctx| cfg.exactly(*ctx))
                    .collect::<Vec<_>>()
                    .map(CborZero::Map),
            );

            let simple = |num: u8| any().filter(move |n| n % (1 << 5) == num);

            let float_simple = choice((
                simple(20).to(CborZero::Bool(false)),
                simple(21).to(CborZero::Bool(true)),
                simple(22).to(CborZero::Null),
                simple(23).to(CborZero::Undef),
                simple(26).ignore_then(
                    any()
                        .repeated()
                        .collect_exactly()
                        .map(f32::from_be_bytes)
                        .map(CborZero::SingleFloat),
                ),
                simple(27).ignore_then(
                    any()
                        .repeated()
                        .collect_exactly()
                        .map(f64::from_be_bytes)
                        .map(CborZero::DoubleFloat),
                ),
            ));

            recursive(|value| {
                let major =
                    |num: u8, bits: u8| any().rewind().filter(move |n| n >> (8 - bits) == num);

                let tag = major(6, 3)
                    .ignore_then(read_uint)
                    .then(value)
                    .map(|(tag, value)| CborZero::Tag(tag, Box::new(value)));

                choice((
                    major(0, 3).ignore_then(uint),
                    major(1, 3).ignore_then(nint),
                    major(2, 3).ignore_then(bstr),
                    major(3, 3).ignore_then(str),
                    major(4, 3).ignore_then(array),
                    major(5, 3).ignore_then(map),
                    major(6, 3).ignore_then(tag),
                    major(7, 3).ignore_then(float_simple),
                ))
            })
        })
    }
}
