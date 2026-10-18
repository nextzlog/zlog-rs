//! Bands, modes, powers and contest modes.
//!
//! The discriminants match the byte values that zLog stores in ZLO/ZLOX files.

macro_rules! byte_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $value:expr => $label:expr),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        #[repr(u8)]
        pub enum $name {
            $($variant = $value),+
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            pub fn from_byte(b: u8) -> Option<Self> {
                match b {
                    $($value => Some($name::$variant),)+
                    _ => None,
                }
            }

            pub fn label(self) -> &'static str {
                match self {
                    $($name::$variant => $label),+
                }
            }

            pub fn from_label(s: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|v| v.label().eq_ignore_ascii_case(s))
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.label())
            }
        }
    };
}

byte_enum! {
    /// Operating mode (`TMode`).
    Mode {
        Cw = 0 => "CW",
        Ssb = 1 => "SSB",
        Fm = 2 => "FM",
        Am = 3 => "AM",
        Rtty = 4 => "RTTY",
        Ft4 = 5 => "FT4",
        Ft8 = 6 => "FT8",
        Other = 7 => "Other",
        Dv = 8 => "DV",
    }
}

byte_enum! {
    /// Band (`TBand`); labels follow `MHzString`.
    Band {
        B1_9 = 0 => "1.9",
        B3_5 = 1 => "3.5",
        B7 = 2 => "7",
        B10 = 3 => "10",
        B14 = 4 => "14",
        B18 = 5 => "18",
        B21 = 6 => "21",
        B24 = 7 => "24",
        B28 = 8 => "28",
        B50 = 9 => "50",
        B144 = 10 => "144",
        B430 = 11 => "430",
        B1200 = 12 => "1200",
        B2400 = 13 => "2400",
        B5600 = 14 => "5600",
        B10G = 15 => "10.1G",
        B10_4G = 16 => "10.4G",
        B24G = 17 => "24G",
        B47G = 18 => "47G",
        B77G = 19 => "77G",
        B135G = 20 => "135G",
        B248G = 21 => "248G",
        Target = 22 => "Target",
        Unknown = 23 => "Unknown",
    }
}

byte_enum! {
    /// Transmitter power class (`TPower`).
    Power {
        P001 = 0 => "1W",
        P002 = 1 => "2W",
        P005 = 2 => "5W",
        P010 = 3 => "10W",
        P020 = 4 => "20W",
        P025 = 5 => "25W",
        P050 = 6 => "50W",
        P100 = 7 => "100W",
        P200 = 8 => "200W",
        P500 = 9 => "500W",
        P1000 = 10 => "1kW",
    }
}

impl Power {
    /// The JARL power code this power class stands for (zLog `NewPowerString`):
    /// P, L, M, H for the four lowest classes, none for the others.
    pub fn code(self) -> Option<char> {
        match self {
            Power::P001 => Some('P'),
            Power::P002 => Some('L'),
            Power::P005 => Some('M'),
            Power::P010 => Some('H'),
            _ => None,
        }
    }

    pub fn from_code(c: char) -> Option<Power> {
        [Power::P001, Power::P002, Power::P005, Power::P010]
            .into_iter()
            .find(|p| p.code() == Some(c))
    }

    /// The next power code for Shift+P: H, P, L, M, then H again.
    pub fn next_code(self) -> Power {
        match self {
            Power::P010 => Power::P001,
            Power::P001 => Power::P002,
            Power::P002 => Power::P005,
            _ => Power::P010,
        }
    }
}

impl Band {
    /// The highest real band (`HiBand`).
    pub const HIGHEST: Band = Band::B248G;

    /// WARC bands (10, 18 and 24 MHz) are excluded from JARL contests.
    pub fn is_warc(self) -> bool {
        matches!(self, Band::B10 | Band::B18 | Band::B24)
    }

    /// Bands from `low` to `high`, both inclusive.
    pub fn range(low: Band, high: Band) -> impl Iterator<Item = Band> {
        Band::ALL
            .iter()
            .copied()
            .filter(move |b| *b >= low && *b <= high)
    }
}

/// Mode category of a contest (`TContestMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContestMode {
    Mix,
    Cw,
    Phone,
    Rtty,
    All,
}

impl ContestMode {
    /// Modes that count in this category (`ContestModeSet`).
    pub fn accepts(self, mode: Mode) -> bool {
        use Mode::*;
        match self {
            ContestMode::Mix => matches!(mode, Cw | Ssb | Fm | Am | Dv),
            ContestMode::Cw => mode == Cw,
            ContestMode::Phone => matches!(mode, Ssb | Fm | Am | Dv),
            ContestMode::Rtty => mode == Rtty,
            ContestMode::All => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_round_trip() {
        for b in Band::ALL {
            assert_eq!(Band::from_byte(*b as u8), Some(*b));
        }
        assert_eq!(Band::from_byte(24), None);
        assert_eq!(Mode::from_label("ssb"), Some(Mode::Ssb));
    }

    #[test]
    fn band_range_is_inclusive() {
        let bands: Vec<_> = Band::range(Band::B50, Band::B430).collect();
        assert_eq!(bands, vec![Band::B50, Band::B144, Band::B430]);
    }
}
