//! Prop values that are either fixed or reactive.

use mosaic::prelude::{Derived, ReadState, State};

/// A prop value that is fixed or follows a [`State`]/[`Derived`]. Reads
/// through [`get`](Self::get) track the reactive source, so bindings that
/// read it re-run when it changes.
pub struct Live<T: 'static>(Source<T>);

enum Source<T: 'static> {
    Fixed(T),
    Read(ReadState<T>),
}

impl<T: Clone + 'static> Clone for Live<T> {
    fn clone(&self) -> Self {
        match &self.0 {
            Source::Fixed(value) => Live(Source::Fixed(value.clone())),
            Source::Read(read) => Live(Source::Read(*read)),
        }
    }
}

impl<T: Clone + 'static> Live<T> {
    pub fn fixed(value: T) -> Self {
        Live(Source::Fixed(value))
    }

    /// The current value; tracked when reactive.
    pub fn get(&self) -> T {
        match &self.0 {
            Source::Fixed(value) => value.clone(),
            Source::Read(read) => read.get(),
        }
    }

    /// The current value without tracking.
    pub fn get_untracked(&self) -> T {
        match &self.0 {
            Source::Fixed(value) => value.clone(),
            Source::Read(read) => read.get_untracked(),
        }
    }
}

impl<T: 'static> From<State<T>> for Live<T> {
    fn from(state: State<T>) -> Self {
        Live(Source::Read(state.into()))
    }
}

impl<T: 'static> From<Derived<T>> for Live<T> {
    fn from(derived: Derived<T>) -> Self {
        Live(Source::Read(derived.into()))
    }
}

impl<T: 'static> From<ReadState<T>> for Live<T> {
    fn from(read: ReadState<T>) -> Self {
        Live(Source::Read(read))
    }
}

macro_rules! fixed_from {
    ($($ty:ty),*) => {$(
        impl From<$ty> for Live<$ty> {
            fn from(value: $ty) -> Self {
                Live(Source::Fixed(value))
            }
        }
    )*};
}

fixed_from!(bool, u8, u32, usize, f32, String);

impl From<&str> for Live<String> {
    fn from(value: &str) -> Self {
        Live(Source::Fixed(value.to_owned()))
    }
}

/// Text that is fixed or reactive: `text:"Plan"` or `text:(state)`.
pub type UiText = Live<String>;

/// Implements `From<T> for Live<T>` for a library value type.
macro_rules! live_value {
    ($ty:ty) => {
        impl From<$ty> for $crate::Live<$ty> {
            fn from(value: $ty) -> Self {
                $crate::Live::fixed(value)
            }
        }
    };
}
pub(crate) use live_value;
