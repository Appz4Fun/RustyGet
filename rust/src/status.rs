//! Status enums of downloads, history records and post-processing.
//!
//! The queue objects, the disk state and the RPC structs keep these as
//! `i32` with NZBGet's numeric values; Rust code passes the enums and
//! converts only at those boundaries (`i32::from`, `TryFrom<i32>`, or
//! `from_raw` where an unknown stored value reads as the zero variant, as
//! the RPC name tables already do).

/// An `i32` that names no variant of the enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnknownStatus(pub i32);

macro_rules! status_enum {
    (
        $(#[$meta:meta])*
        $name:ident { $($(#[$vmeta:meta])* $variant:ident = $value:literal,)+ }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(i32)]
        pub enum $name {
            #[default]
            $($(#[$vmeta])* $variant = $value,)+
        }

        impl $name {
            pub const ALL: &'static [Self] = &[$(Self::$variant,)+];

            /// The stored value as an enum; an unknown value reads as the
            /// zero variant.
            pub fn from_raw(v: i32) -> Self {
                Self::try_from(v).unwrap_or_default()
            }
        }

        impl From<$name> for i32 {
            fn from(v: $name) -> i32 {
                v as i32
            }
        }

        impl TryFrom<i32> for $name {
            type Error = UnknownStatus;
            fn try_from(v: i32) -> Result<Self, UnknownStatus> {
                match v {
                    $($value => Ok(Self::$variant),)+
                    _ => Err(UnknownStatus(v)),
                }
            }
        }
    };
}

status_enum! {
    /// A download's par-check outcome (NzbInfo::EParStatus).
    NzbParStatus {
        None = 0,
        Skipped = 1,
        Failure = 2,
        Success = 3,
        RepairPossible = 4,
        Manual = 5,
    }
}

status_enum! {
    /// What a par check found (ParChecker::EStatus).
    ParCheckerStatus {
        Failed = 0,
        RepairPossible = 1,
        Repaired = 2,
        RepairNotNeeded = 3,
    }
}

status_enum! {
    /// A download's unpack outcome (NzbInfo::EUnpackStatus).
    UnpackStatus {
        None = 0,
        Skipped = 1,
        Failure = 2,
        Success = 3,
        Space = 4,
        Password = 5,
    }
}

status_enum! {
    /// Whether the files were moved to the final directory
    /// (NzbInfo::EMoveStatus).
    MoveStatus {
        None = 0,
        Failure = 1,
        Success = 2,
    }
}

status_enum! {
    /// Whether the extension cleanup ran (NzbInfo::ECleanupStatus).
    CleanupStatus {
        None = 0,
        Failure = 1,
        Success = 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<T>(all: &[T])
    where
        T: Copy + std::fmt::Debug + PartialEq + Into<i32> + TryFrom<i32, Error = UnknownStatus>,
    {
        for &v in all {
            let raw: i32 = v.into();
            assert_eq!(T::try_from(raw), Ok(v));
        }
        let max = all.iter().map(|&v| v.into()).max().unwrap();
        assert_eq!(T::try_from(max + 1), Err(UnknownStatus(max + 1)));
        assert_eq!(T::try_from(-1), Err(UnknownStatus(-1)));
    }

    #[test]
    fn par_statuses_round_trip() {
        round_trip(NzbParStatus::ALL);
        round_trip(ParCheckerStatus::ALL);
        assert_eq!(i32::from(NzbParStatus::RepairPossible), 4);
        assert_eq!(i32::from(ParCheckerStatus::RepairNotNeeded), 3);
        assert_eq!(NzbParStatus::from_raw(99), NzbParStatus::None);
    }

    #[test]
    fn unpack_move_cleanup_statuses_round_trip() {
        round_trip(UnpackStatus::ALL);
        round_trip(MoveStatus::ALL);
        round_trip(CleanupStatus::ALL);
        assert_eq!(i32::from(UnpackStatus::Password), 5);
        assert_eq!(i32::from(MoveStatus::Success), 2);
        assert_eq!(i32::from(CleanupStatus::Failure), 1);
        assert_eq!(UnpackStatus::from_raw(-3), UnpackStatus::None);
    }
}
