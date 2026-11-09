use compatdb_core::{Argument, D3d9, Dxgi, EnvEntry, EnvName, Exe, Rule, Table, check_env_name};

use crate::Result;

/// D3D9 implementation for a program.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum D3d9Backend {
    /// mtld3d: D3D9 directly on Metal. The engine's default.
    Mtld3d,
    /// Wine's wined3d: slower, more conformant.
    Wined3d,
}

/// D3D10/11/12 implementation for a program.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DxgiBackend {
    /// DXMT for D3D10/11, vkd3d-proton for D3D12. The engine's default.
    Dxmt,
    /// Wine's wined3d (D3D10/11 only).
    Wined3d,
}

impl D3d9Backend {
    /// The engine's name for it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Mtld3d => "mtld3d",
            Self::Wined3d => "wined3d",
        }
    }

    /// Parses `mtld3d` or `wined3d`.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "mtld3d" => Some(Self::Mtld3d),
            "wined3d" => Some(Self::Wined3d),
            _ => None,
        }
    }
}

impl DxgiBackend {
    /// The engine's name for it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Dxmt => "dxmt",
            Self::Wined3d => "wined3d",
        }
    }

    /// Parses `dxmt` or `wined3d`.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "dxmt" => Some(Self::Dxmt),
            "wined3d" => Some(Self::Wined3d),
            _ => None,
        }
    }
}

/// What crossfree knows about one program. This is crossfree's own model
/// (what its compatibility database stores); [`GameProfile::to_rule`] turns
/// it into the engine's rule format.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GameProfile {
    /// Executable basename (`Game.exe`), or `steam:<appid>`.
    pub exe: String,
    /// D3D9 implementation, or the engine's default.
    pub d3d9: Option<D3d9Backend>,
    /// D3D10–12 implementation, or the engine's default.
    pub dxgi: Option<DxgiBackend>,
    /// Environment variables for this program only (`NAME`, `value`).
    pub env: Vec<(String, String)>,
    /// Extra command-line arguments.
    pub arguments: Vec<String>,
}

impl GameProfile {
    /// A profile for `exe` with nothing set.
    pub fn new(exe: impl Into<String>) -> Self {
        Self { exe: exe.into(), ..Self::default() }
    }

    /// The engine rule for this profile.
    pub fn to_rule(&self) -> Result<Rule> {
        let mut rule = Rule::new(Exe::new(self.exe.as_str())?);
        if let Some(d3d9) = self.d3d9 {
            rule = rule.with_d3d9(match d3d9 {
                D3d9Backend::Mtld3d => D3d9::Mtld3d,
                D3d9Backend::Wined3d => D3d9::Wined3d,
            });
        }
        if let Some(dxgi) = self.dxgi {
            rule = rule.with_dxgi(match dxgi {
                DxgiBackend::Dxmt => Dxgi::Dxmt,
                DxgiBackend::Wined3d => Dxgi::Wined3d,
            });
        }
        for (name, value) in &self.env {
            // The typed builder does not refuse variables a setting replaced
            // (such as WINEDLLOVERRIDES); the launcher contract requires it.
            check_env_name(name)?;
            rule = rule.with_env(EnvEntry::new(EnvName::new(name.as_str())?, value.as_str())?);
        }
        for argument in &self.arguments {
            rule = rule.with_argument(Argument::new(argument.as_str())?);
        }
        Ok(rule)
    }
}

/// The engine rules table for a set of profiles, in order.
pub fn rules(profiles: &[GameProfile]) -> Result<Table> {
    let mut table = Table::new();
    for profile in profiles {
        let rule = profile.to_rule()?;
        if !rule.sets_nothing() {
            table.push(rule);
        }
    }
    Ok(table)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_become_engine_rules() {
        let mut game = GameProfile::new("RiseOfNations.exe");
        game.d3d9 = Some(D3d9Backend::Wined3d);
        game.env.push(("FEX_HARDWARE_TSO".into(), "0".into()));
        let mut other = GameProfile::new("steam:287450");
        other.dxgi = Some(DxgiBackend::Dxmt);
        other.arguments.push("-windowed".into());
        let table = rules(&[game, other, GameProfile::new("Nothing.exe")]).unwrap();
        assert_eq!(
            table.to_string(),
            "exe=RiseOfNations.exe;d3d9=wined3d;env=FEX_HARDWARE_TSO=0\n\
             exe=steam:287450;dxgi=dxmt;arguments=-windowed"
        );
    }

    #[test]
    fn rejects_invalid_settings() {
        assert!(GameProfile::new("").to_rule().is_err());
        assert!(GameProfile::new("steam:0").to_rule().is_err());
        let mut bad_env = GameProfile::new("Game.exe");
        bad_env.env.push(("WINEDLLOVERRIDES".into(), "x=n".into()));
        assert!(bad_env.to_rule().is_err(), "a variable a setting replaced must be refused");
    }

    #[test]
    fn backend_names() {
        assert_eq!(D3d9Backend::from_name("wined3d"), Some(D3d9Backend::Wined3d));
        assert_eq!(D3d9Backend::from_name("gptk"), None);
        assert_eq!(DxgiBackend::from_name("dxmt"), Some(DxgiBackend::Dxmt));
        assert_eq!(DxgiBackend::from_name("dxvk"), None);
        for b in [D3d9Backend::Mtld3d, D3d9Backend::Wined3d] {
            assert_eq!(D3d9Backend::from_name(b.name()), Some(b));
        }
        for b in [DxgiBackend::Dxmt, DxgiBackend::Wined3d] {
            assert_eq!(DxgiBackend::from_name(b.name()), Some(b));
        }
    }
}
