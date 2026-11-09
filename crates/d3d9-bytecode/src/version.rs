use std::fmt;

/// Pipeline stage a shader belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShaderKind {
    /// Vertex shader (`vs_*`).
    Vertex,
    /// Pixel shader (`ps_*`).
    Pixel,
}

/// Minor version used by the software profiles (`vs_2_sw`, `ps_3_sw`, ...).
const SW_MINOR: u8 = 0xff;

/// Shader version, decoded from the first token of the bytecode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Version {
    /// Pipeline stage.
    pub kind: ShaderKind,
    /// Major version, 1 to 3.
    pub major: u8,
    /// Minor version. `vs_2_x` / `ps_2_x` use 1; software profiles use 0xff.
    pub minor: u8,
}

impl Version {
    /// Decodes a version token, or returns `None` if `token` is not one.
    pub fn from_token(token: u32) -> Option<Self> {
        let kind = match token >> 16 {
            0xfffe => ShaderKind::Vertex,
            0xffff => ShaderKind::Pixel,
            _ => return None,
        };
        Some(Self { kind, major: (token >> 8) as u8, minor: token as u8 })
    }

    /// Encodes the version as a token.
    pub fn to_token(self) -> u32 {
        let high: u32 = match self.kind {
            ShaderKind::Vertex => 0xfffe,
            ShaderKind::Pixel => 0xffff,
        };
        (high << 16) | (u32::from(self.major) << 8) | u32::from(self.minor)
    }

    /// Whether Direct3D 9 defines this version.
    pub fn is_supported(self) -> bool {
        matches!(
            (self.kind, self.major, self.minor),
            (ShaderKind::Vertex, 1, 0 | 1)
                | (ShaderKind::Pixel, 1, 0..=4)
                | (_, 2, 0 | 1 | SW_MINOR)
                | (_, 3, 0 | SW_MINOR)
        )
    }

    /// Whether this version is `major.minor` or later.
    pub fn at_least(self, major: u8, minor: u8) -> bool {
        (self.major, self.minor) >= (major, minor)
    }

    /// Whether this version is `major.minor` or earlier.
    pub fn at_most(self, major: u8, minor: u8) -> bool {
        (self.major, self.minor) <= (major, minor)
    }

    /// Whether this version is exactly `major.minor`.
    pub fn is(self, major: u8, minor: u8) -> bool {
        (self.major, self.minor) == (major, minor)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let prefix = match self.kind {
            ShaderKind::Vertex => "vs",
            ShaderKind::Pixel => "ps",
        };
        match (self.major, self.minor) {
            (2, 1) => write!(f, "{prefix}_2_x"),
            (major, SW_MINOR) => write!(f, "{prefix}_{major}_sw"),
            (major, minor) => write!(f, "{prefix}_{major}_{minor}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_and_encodes_tokens() {
        let v = Version::from_token(0xfffe_0101).unwrap();
        assert_eq!((v.kind, v.major, v.minor), (ShaderKind::Vertex, 1, 1));
        assert_eq!(v.to_token(), 0xfffe_0101);
        let p = Version::from_token(0xffff_0300).unwrap();
        assert_eq!((p.kind, p.major, p.minor), (ShaderKind::Pixel, 3, 0));
        assert_eq!(p.to_token(), 0xffff_0300);
        assert_eq!(Version::from_token(0x0000_0101), None);
    }

    #[test]
    fn knows_which_versions_exist() {
        let ok = [
            0xfffe_0100,
            0xfffe_0101,
            0xfffe_0200,
            0xfffe_0201,
            0xfffe_02ff,
            0xfffe_0300,
            0xfffe_03ff,
            0xffff_0100,
            0xffff_0104,
            0xffff_0200,
            0xffff_0300,
        ];
        for token in ok {
            assert!(Version::from_token(token).unwrap().is_supported(), "{token:#x}");
        }
        for token in [0xfffe_0102, 0xfffe_0104, 0xffff_0105, 0xffff_0202, 0xffff_0400] {
            assert!(!Version::from_token(token).unwrap().is_supported(), "{token:#x}");
        }
    }

    #[test]
    fn formats_like_the_assembler() {
        let name = |t| Version::from_token(t).unwrap().to_string();
        assert_eq!(name(0xfffe_0101), "vs_1_1");
        assert_eq!(name(0xffff_0104), "ps_1_4");
        assert_eq!(name(0xffff_0201), "ps_2_x");
        assert_eq!(name(0xfffe_02ff), "vs_2_sw");
        assert_eq!(name(0xfffe_0300), "vs_3_0");
    }
}
