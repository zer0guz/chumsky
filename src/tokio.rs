use crate::input::{CacheOf, CursorOf, InputFor, MaybeTokenOf, SliceInputFor, SliceOf};

use super::*;

use bytes::Bytes;


impl<'src> InputFor<'src> for Bytes {
    type Cursor = usize;
    type Span = SimpleSpan<usize>;

    type Token = u8;
    type MaybeToken = u8;

    type Cache = Self;

}

impl Input for Bytes {
  
    #[inline]
    fn begin<'src>(self) -> (CursorOf<'src,Self>, CacheOf<'src,Self>) {
        (0, self)
    }

    #[inline]
    fn cursor_location<'src>(cursor: &CursorOf<'src,Self>) -> usize {
        *cursor
    }

    #[inline(always)]
    unsafe fn next_maybe<'src>(
        this: &mut CacheOf<'src,Self>,
        cursor: &mut CursorOf<'src,Self>,
    ) -> Option<MaybeTokenOf<'src,Self>> {
        if let Some(tok) = this.get(*cursor) {
            *cursor += 1;
            Some(*tok)
        } else {
            None
        }
    }

    #[inline(always)]
    unsafe fn span<'src>(_this: &mut CacheOf<'src,Self>, range: Range<&CursorOf<'src,Self>>) -> SpanOf<'src,Self> {
        (*range.start..*range.end).into()
    }
}

impl ExactSizeInput for Bytes {
    #[inline(always)]
    unsafe fn span_from<'src>(this: &mut CacheOf<'src,Self>, range: RangeFrom<&CursorOf<'src,Self>>) -> SpanOf<'src,Self> {
        (*range.start..this.len()).into()
    }
}


impl<'src> SliceInputFor<'src> for Bytes {
    type Slice = Bytes;
}

impl Sealed for Bytes {}
impl<'src> StrInput for Bytes {
    #[doc(hidden)]
    fn stringify(slice: SliceOf<'src,Self>) -> String {
        slice
            .iter()
            // .map(|e| core::ascii::Char::from_u8(e).unwrap_or(AsciiChar::Substitute).to_char())
            .map(|e| char::from(*e))
            .collect()
    }
}

impl SliceInput for Bytes {

    #[inline(always)]
    fn full_slice<'src>(this: &mut CacheOf<'src,Self>) -> SliceOf<'src,Self> {
        this.clone()
    }

    #[inline(always)]
    unsafe fn slice<'src>(this: &mut CacheOf<'src,Self>, range: Range<&CursorOf<'src,Self>>) -> SliceOf<'src,Self> {
        this.slice(*range.start..*range.end)
    }

    #[inline(always)]
    unsafe fn slice_from<'src>(this: &mut CacheOf<'src,Self>, from: RangeFrom<&CursorOf<'src,Self>>) -> SliceOf<'src,Self> {
        this.slice(*from.start..)
    }
}

impl ValueInput for Bytes {
    #[inline(always)]
    unsafe fn next<'src>(this: &mut CacheOf<'src,Self>, cursor: &mut CursorOf<'src,Self>) -> Option<TokenOf<'src,Self>> {
        unsafe { Self::next_maybe(this, cursor) }
    }
}
