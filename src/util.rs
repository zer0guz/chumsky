//! Utility items used throughout the crate.

use super::*;

use core::hash::Hasher;

pub struct Ref<'r, T: ?Sized> {
    ptr: core::ptr::NonNull<T>,
    _lt: core::marker::PhantomData<&'r ()>,
}

impl<'r, T: ?Sized> Ref<'r, T> {
    #[inline(always)]
    pub fn into_ref(self) -> &'r T {
        // SAFETY: `Ref` is only constructed from a valid `&'r T` (or equivalent),
        // so the pointer is valid for `'r`.
        unsafe { self.ptr.as_ref() }
    }

    #[inline(always)]
    pub fn as_ptr(self) -> *const T {
        self.ptr.as_ptr()
    }

    #[inline(always)]
    pub unsafe fn from_ptr(ptr: *const T) -> Self {
        Self {
            ptr: unsafe { core::ptr::NonNull::new_unchecked(ptr as *mut T) },
            _lt: core::marker::PhantomData,
        }
    }
}
impl<'src, T, S> Ref<'src, (T, S)> {
    #[inline(always)]
    pub fn split(self) -> (Ref<'src, T>, Ref<'src, S>) {
        unsafe {
            let base = self.as_ptr();
            (
                Ref::from_ptr(core::ptr::addr_of!((*base).0)),
                Ref::from_ptr(core::ptr::addr_of!((*base).1)),
            )
        }
    }
}

pub fn split_ref<'src,'map, T, S>(r: Ref<'src, (T, S)>,lt: &'map()) -> (Ref<'src, T>, Ref<'src, S>) {
    r.split()
}

impl<'r, T: ?Sized> Borrow<T> for Ref<'r, T> {
    fn borrow(&self) -> &T {
        self
    }
}

impl<T: ?Sized> Clone for Ref<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: ?Sized> Copy for Ref<'_, T> {}

impl<T: ?Sized> ::core::ops::Deref for Ref<'_, T> {
    #[inline]
    fn deref(&self) -> &T {
        unsafe { self.ptr.as_ref() }
    }
    type Target = T;
}

impl<T: ?Sized> ::core::fmt::Debug for Ref<'_, T>
where
    T: ::core::fmt::Debug,
{
    fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
        T::fmt(self, f)
    }
}

impl<'r, T: ?Sized> From<&'r T> for Ref<'r, T> {
    fn from(r: &'r T) -> Self {
        Self {
            ptr: core::ptr::NonNull::from(r),
            _lt: core::marker::PhantomData,
        }
    }
}

mod fn_traits {
    pub trait FnOnce<Args> {
        type Output;
    }

    impl<F, R> FnOnce<()> for F
    where
        F: ::core::ops::FnOnce() -> R,
    {
        type Output = R;
    }
}

pub type Never = <fn() -> ! as fn_traits::FnOnce<()>>::Output;

// Reverse `From` or `Into<&'r T>` impl.

pub enum Maybe2<'r, T, R: Deref<Target = T>> {
    Owned(T),
    Ref(R),
    _PhantomVariant(PhantomData<&'r ()>, Never),
}

/// A value that may be a `T` or a mutable reference to a `T`.
pub type MaybeMut<'a, T> = Maybe<'a, T, &'a mut T>;

/// A value that may be a `T` or a shared reference to a `T`.
pub type MaybeRef<'a, T> = Maybe<'a, T, Ref<'a, T>>;

/// A type that can represent a borrowed reference to a `T` or a value of `T`.
///
/// Used internally to facilitate zero-copy manipulation of tokens during error generation (see [`Error`]).
#[derive(Copy, Clone)]
pub enum Maybe<'r, T, R: Deref<Target = T>> {
    /// We have a reference to `T`.
    Ref(R),
    /// We have a value of `T`.
    Val(T),
    _PhantomVariant(PhantomData<&'r ()>, Never),
}

impl<T: PartialEq, R: Deref<Target = T>> PartialEq for Maybe<'_, T, R> {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl<T: Eq, R: Deref<Target = T>> Eq for Maybe<'_, T, R> {}

impl<T: PartialOrd, R: Deref<Target = T>> PartialOrd for Maybe<'_, T, R> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        (**self).partial_cmp(&**other)
    }
}

impl<T: Ord, R: Deref<Target = T>> Ord for Maybe<'_, T, R> {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        (**self).cmp(&**other)
    }
}

impl<T: Hash, R: Deref<Target = T>> Hash for Maybe<'_, T, R> {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        T::hash(&**self, state)
    }
}

impl<T: fmt::Debug, R: Deref<Target = T>> fmt::Debug for Maybe<'_, T, R> {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        T::fmt(&**self, f)
    }
}

impl<T, R: Deref<Target = T>> Maybe<'_, T, R> {
    /// Convert this [`Maybe<T, _>`] into a `T`, cloning the inner value if necessary.
    #[inline]
    pub fn into_inner(self) -> T
    where
        T: Clone,
    {
        match self {
            Self::Ref(x) => x.clone(),
            Self::Val(x) => x,
        }
    }

    /// Convert this [`Maybe<T, _>`] into an owned version of itself, cloning the inner reference if required.
    #[inline]
    pub fn into_owned<U>(self) -> Maybe<'static, T, U>
    where
        T: Clone,
        U: Deref<Target = T>,
    {
        Maybe::Val(self.into_inner())
    }
}

impl<T, R: Deref<Target = T>> Deref for Maybe<'_, T, R> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Ref(x) => x,
            Self::Val(x) => x,
            Self::_PhantomVariant(_, _) => unreachable!(),
        }
    }
}

impl<T, R: DerefMut<Target = T>> DerefMut for Maybe<'_, T, R> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::Ref(x) => &mut *x,
            Self::Val(x) => x,
            Self::_PhantomVariant(_, _) => unreachable!(),
        }
    }
}

impl<T> From<T> for Maybe<'_, T, &T> {
    #[inline]
    fn from(x: T) -> Self {
        Self::Val(x)
    }
}

impl<T> From<T> for Maybe<'_, T, &mut T> {
    #[inline]
    fn from(x: T) -> Self {
        Self::Val(x)
    }
}

impl<'a, T> From<&'a T> for Maybe<'a, T, &'a T> {
    #[inline]
    fn from(x: &'a T) -> Self {
        Self::Ref(x)
    }
}

impl<'a, T> From<&'a mut T> for Maybe<'a, T, &'a mut T> {
    #[inline]
    fn from(x: &'a mut T) -> Self {
        Self::Ref(x)
    }
}

impl<'r, T> From<T> for Maybe<'r, T, Ref<'r, T>> {
    #[inline]
    fn from(x: T) -> Self {
        Maybe::Val(x)
    }
}
impl<'a, T> From<&'a T> for Maybe<'a, T, Ref<'a, T>> {
    #[inline]
    fn from(x: &'a T) -> Self {
        Maybe::Ref(Ref::from(x))
    }
}

impl<'r, T: ?Sized> From<Ref<'r, T>> for Maybe<'r, T, Ref<'r, T>>
where
    T: Sized,
{
    #[inline]
    fn from(r: Ref<'r, T>) -> Self {
        Maybe::Ref(r)
    }
}

#[cfg(feature = "serde")]
impl<T: Serialize, R: Deref<Target = T>> Serialize for Maybe<'_, T, R> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_newtype_struct("Maybe", &**self)
    }
}
#[cfg(feature = "serde")]
impl<'de, 'r, T, R> Deserialize<'de> for Maybe<'r, T, R>
where
    T: Deserialize<'de>,
    R: Deref<Target = T>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct MaybeVisitor<'r, T, R>(PhantomData<(&'r (), T, R)>);

        impl<'de2, 'r, T, R> Visitor<'de2> for MaybeVisitor<'r, T, R>
        where
            T: Deserialize<'de2>,
            R: Deref<Target = T>,
        {
            type Value = Maybe<'r, T, R>;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a Maybe")
            }

            fn visit_newtype_struct<D>(self, d: D) -> Result<Self::Value, D::Error>
            where
                D: Deserializer<'de2>,
            {
                // Always deserialize as owned.
                T::deserialize(d).map(Maybe::Val)
            }
        }

        deserializer.deserialize_newtype_struct("Maybe", MaybeVisitor(PhantomData))
    }
}

mod ref_or_val_sealed {
    pub trait Sealed<T> {}
}

/// An trait that allows abstracting over values of or references to a `T`.
///
/// Some [`Input`]s can only generate tokens by-reference (like `&[T]` -> `&T`), and some can only generate tokens
/// by-value (like `&str` -> `char`). This trait allows chumsky to handle both kinds of input.
///
/// The trait is sealed: you cannot implement it yourself.
pub trait IntoMaybe<'src, T>:
    ref_or_val_sealed::Sealed<T> + Borrow<T> + Into<MaybeRef<'src, T>>
{
    /// Project the referential properties of this type on to another type.
    ///
    /// For example, `<&Foo>::Proj<Bar> = &Bar` but `<Foo>::Proj<Bar> = Bar`.
    #[doc(hidden)]
    type Proj<U>: IntoMaybe<'src, U>;
}

impl<T> ref_or_val_sealed::Sealed<T> for Ref<'_, T> {}
impl<'src, T> IntoMaybe<'src, T> for Ref<'src, T> {
    type Proj<U> = Ref<'src, U>;
}

impl<T> ref_or_val_sealed::Sealed<T> for T {}
impl<'src, T> IntoMaybe<'src, T> for T {
    type Proj<U> = U;
}
