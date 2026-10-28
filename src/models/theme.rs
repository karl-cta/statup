//! The look an administrator gives the page: an accent colour and an
//! ambiance (the ground and ink family), each from a closed list.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Accent {
    #[default]
    Ink,
    Navy,
    Cobalt,
    Teal,
    Pine,
    Rose,
    Violet,
    Burgundy,
    Brick,
    Copper,
    Cocoa,
    Slate,
}

impl Accent {
    pub const ALL: [Self; 12] = [
        Self::Ink,
        Self::Navy,
        Self::Cobalt,
        Self::Teal,
        Self::Pine,
        Self::Rose,
        Self::Violet,
        Self::Burgundy,
        Self::Brick,
        Self::Copper,
        Self::Cocoa,
        Self::Slate,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ink => "ink",
            Self::Navy => "navy",
            Self::Cobalt => "cobalt",
            Self::Teal => "teal",
            Self::Pine => "pine",
            Self::Rose => "rose",
            Self::Violet => "violet",
            Self::Burgundy => "burgundy",
            Self::Brick => "brick",
            Self::Copper => "copper",
            Self::Cocoa => "cocoa",
            Self::Slate => "slate",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|accent| accent.as_str() == value)
    }

    /// The value of the `data-accent` attribute, absent for the default.
    pub fn attribute(self) -> Option<&'static str> {
        (self != Self::default()).then(|| self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Ambiance {
    #[default]
    Sand,
    Mist,
    Paper,
    Pebble,
}

impl Ambiance {
    pub const ALL: [Self; 4] = [Self::Sand, Self::Mist, Self::Paper, Self::Pebble];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sand => "sand",
            Self::Mist => "mist",
            Self::Paper => "paper",
            Self::Pebble => "pebble",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|ambiance| ambiance.as_str() == value)
    }

    /// The value of the `data-ambiance` attribute, absent for the default.
    pub fn attribute(self) -> Option<&'static str> {
        (self != Self::default()).then(|| self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_accent_round_trips_through_its_id() {
        for accent in Accent::ALL {
            assert_eq!(Accent::parse(accent.as_str()), Some(accent));
        }
    }

    #[test]
    fn every_ambiance_round_trips_through_its_id() {
        for ambiance in Ambiance::ALL {
            assert_eq!(Ambiance::parse(ambiance.as_str()), Some(ambiance));
        }
    }

    #[test]
    fn parsing_refuses_anything_but_an_exact_id() {
        for value in ["", "neon", "Navy", " navy", "navy ", "NAVY"] {
            assert_eq!(Accent::parse(value), None, "accent {value:?}");
        }
        for value in ["", "neon", "Sand", " sand", "sand ", "SAND"] {
            assert_eq!(Ambiance::parse(value), None, "ambiance {value:?}");
        }
    }

    #[test]
    fn only_the_default_has_no_attribute() {
        for accent in Accent::ALL {
            assert_eq!(accent.attribute().is_none(), accent == Accent::default());
        }
        for ambiance in Ambiance::ALL {
            assert_eq!(
                ambiance.attribute().is_none(),
                ambiance == Ambiance::default()
            );
        }
    }

    #[test]
    fn ids_are_unique() {
        let accents: HashSet<_> = Accent::ALL.iter().map(|accent| accent.as_str()).collect();
        assert_eq!(accents.len(), Accent::ALL.len());
        let ambiances: HashSet<_> = Ambiance::ALL.iter().map(|a| a.as_str()).collect();
        assert_eq!(ambiances.len(), Ambiance::ALL.len());
    }
}
