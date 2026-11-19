//! Status enums of downloads, history records and post-processing.
//!
//! The queue objects, the disk state and the RPC structs keep these as
//! `i32` with NZBGet's numeric values; Rust code passes the enums and
//! converts only at those boundaries, with `From` both ways. The
//! conversion is lossless: a stored value that names no variant (from an
//! old or hand-edited disk state, or a binary RPC client) becomes
//! `Other(value)`, matches none of the named variants, orders by its
//! number as the raw value did, and is written back unchanged.

macro_rules! status_enum {
    (
        $(#[$meta:meta])*
        $name:ident { $first:ident = $first_value:literal, $($(#[$vmeta:meta])* $variant:ident = $value:literal,)* }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug)]
        pub enum $name {
            $first,
            $($(#[$vmeta])* $variant,)*
            /// A stored value that names no variant, kept as it is.
            Other(i32),
        }

        impl $name {
            pub const ALL: &'static [Self] = &[Self::$first, $(Self::$variant,)*];
        }

        impl Default for $name {
            fn default() -> Self {
                Self::$first
            }
        }

        impl From<i32> for $name {
            fn from(v: i32) -> Self {
                match v {
                    $first_value => Self::$first,
                    $($value => Self::$variant,)*
                    _ => Self::Other(v),
                }
            }
        }

        impl From<$name> for i32 {
            fn from(v: $name) -> i32 {
                match v {
                    $name::$first => $first_value,
                    $($name::$variant => $value,)*
                    $name::Other(v) => v,
                }
            }
        }

        impl PartialEq for $name {
            fn eq(&self, other: &Self) -> bool {
                i32::from(*self) == i32::from(*other)
            }
        }

        impl Eq for $name {}

        impl PartialOrd for $name {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }

        impl Ord for $name {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                i32::from(*self).cmp(&i32::from(*other))
            }
        }

        impl std::hash::Hash for $name {
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                i32::from(*self).hash(state);
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

status_enum! {
    /// A hidden history record's dupe outcome (DupInfo::EStatus).
    DupStatus {
        Undefined = 0,
        Success = 1,
        Failed = 2,
        Deleted = 3,
        Dupe = 4,
        Bad = 5,
        Good = 6,
    }
}

/// How a download takes part in duplicate handling (NzbInfo::EDupeMode).
///
/// The RPC, binary RPC and feed entry points store whatever number they are
/// given, so `Other` keeps a value that names no mode: it matches none of
/// the mode checks and converts back unchanged.
#[derive(Clone, Copy, Debug, Default)]
pub enum DupeMode {
    #[default]
    Score,
    All,
    Force,
    Other(i32),
}

impl DupeMode {
    pub const ALL: &'static [Self] = &[Self::Score, Self::All, Self::Force];
}

impl From<i32> for DupeMode {
    fn from(v: i32) -> Self {
        match v {
            0 => Self::Score,
            1 => Self::All,
            2 => Self::Force,
            _ => Self::Other(v),
        }
    }
}

impl From<DupeMode> for i32 {
    fn from(v: DupeMode) -> i32 {
        match v {
            DupeMode::Score => 0,
            DupeMode::All => 1,
            DupeMode::Force => 2,
            DupeMode::Other(v) => v,
        }
    }
}

impl PartialEq for DupeMode {
    fn eq(&self, other: &Self) -> bool {
        i32::from(*self) == i32::from(*other)
    }
}

impl Eq for DupeMode {}

status_enum! {
    /// How the user marked a download (NzbInfo::EMarkStatus).
    MarkStatus {
        None = 0,
        Bad = 1,
        Good = 2,
        Success = 3,
    }
}

status_enum! {
    /// A URL download's state (NzbInfo::EUrlStatus).
    UrlStatus {
        None = 0,
        Running = 1,
        Finished = 2,
        Failed = 3,
        Retry = 4,
        ScanSkipped = 5,
        ScanFailed = 6,
    }
}

status_enum! {
    /// A post-processing script's outcome (ScriptStatus::EStatus).
    ScriptStatus {
        None = 0,
        Failure = 1,
        Success = 2,
    }
}

status_enum! {
    /// Why a download was deleted (NzbInfo::EDeleteStatus).
    DeleteStatus {
        None = 0,
        Manual = 1,
        Health = 2,
        Dupe = 3,
        Bad = 4,
        Good = 5,
        Copy = 6,
        Scan = 7,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every variant, and values that name none, survive `i32 -> enum -> i32`;
    /// an unknown value equals no variant and orders by its number.
    fn round_trip<T>(all: &[T])
    where
        T: Copy + std::fmt::Debug + Ord + Into<i32> + From<i32>,
    {
        for &v in all {
            let raw: i32 = v.into();
            assert_eq!(T::from(raw), v);
        }
        let max = all.iter().map(|&v| v.into()).max().unwrap();
        for raw in [max + 1, -1, 99] {
            let v = T::from(raw);
            assert_eq!(v.into(), raw);
            assert!(all.iter().all(|&known| known != v));
        }
        assert!(T::from(max + 1) > T::from(max));
        assert!(T::from(-1) < T::from(0));
    }

    #[test]
    fn par_statuses_round_trip() {
        round_trip(NzbParStatus::ALL);
        round_trip(ParCheckerStatus::ALL);
        assert_eq!(i32::from(NzbParStatus::RepairPossible), 4);
        assert_eq!(i32::from(ParCheckerStatus::RepairNotNeeded), 3);
        assert_eq!(NzbParStatus::from(99), NzbParStatus::Other(99));
        assert_eq!(NzbParStatus::Other(3), NzbParStatus::Success);
        assert_eq!(NzbParStatus::default(), NzbParStatus::None);
    }

    #[test]
    fn unpack_move_cleanup_statuses_round_trip() {
        round_trip(UnpackStatus::ALL);
        round_trip(MoveStatus::ALL);
        round_trip(CleanupStatus::ALL);
        assert_eq!(i32::from(UnpackStatus::Password), 5);
        assert_eq!(i32::from(MoveStatus::Success), 2);
        assert_eq!(i32::from(CleanupStatus::Failure), 1);
        assert_eq!(i32::from(UnpackStatus::from(-3)), -3);
    }

    #[test]
    fn dup_status_round_trip() {
        round_trip(DupStatus::ALL);
        assert_eq!(i32::from(DupStatus::Undefined), 0);
        assert_eq!(i32::from(DupStatus::Dupe), 4);
        assert_eq!(i32::from(DupStatus::Good), 6);
        assert_eq!(i32::from(DupStatus::from(7)), 7);
    }

    #[test]
    fn dupe_mode_round_trip() {
        for &m in DupeMode::ALL {
            assert_eq!(DupeMode::from(i32::from(m)), m);
        }
        assert_eq!(i32::from(DupeMode::Score), 0);
        assert_eq!(i32::from(DupeMode::All), 1);
        assert_eq!(i32::from(DupeMode::Force), 2);
        for raw in [3, -1] {
            let m = DupeMode::from(raw);
            assert_eq!(m, DupeMode::Other(raw));
            assert_eq!(i32::from(m), raw);
            assert!(DupeMode::ALL.iter().all(|&known| known != m));
        }
        assert_eq!(DupeMode::default(), DupeMode::Score);
    }

    #[test]
    fn mark_status_round_trip() {
        round_trip(MarkStatus::ALL);
        assert_eq!(i32::from(MarkStatus::Bad), 1);
        assert_eq!(i32::from(MarkStatus::Success), 3);
        assert_eq!(MarkStatus::from(4), MarkStatus::Other(4));
    }

    #[test]
    fn url_and_script_statuses_round_trip() {
        round_trip(UrlStatus::ALL);
        round_trip(ScriptStatus::ALL);
        assert_eq!(i32::from(UrlStatus::Retry), 4);
        assert_eq!(i32::from(UrlStatus::ScanFailed), 6);
        assert_eq!(i32::from(ScriptStatus::Success), 2);
        assert_eq!(UrlStatus::from(7), UrlStatus::Other(7));
        assert_eq!(ScriptStatus::from(-1), ScriptStatus::Other(-1));
    }

    #[test]
    fn delete_status_round_trip() {
        round_trip(DeleteStatus::ALL);
        assert_eq!(i32::from(DeleteStatus::Health), 2);
        assert_eq!(i32::from(DeleteStatus::Copy), 6);
        assert_eq!(i32::from(DeleteStatus::Scan), 7);
        assert_eq!(DeleteStatus::from(8), DeleteStatus::Other(8));
        assert!(DeleteStatus::None < DeleteStatus::Manual);
    }
}
