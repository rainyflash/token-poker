use std::{fmt, str::FromStr};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolunteerConsent {
    Undecided,
    Granted,
    Declined,
}

impl VolunteerConsent {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Undecided => "undecided",
            Self::Granted => "granted",
            Self::Declined => "declined",
        }
    }
}

impl FromStr for VolunteerConsent {
    type Err = VolunteerInputError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "undecided" => Ok(Self::Undecided),
            "granted" => Ok(Self::Granted),
            "declined" => Ok(Self::Declined),
            _ => Err(VolunteerInputError::Consent(value.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostNetworkCost {
    Unmetered,
    Metered,
    Unknown,
}

impl HostNetworkCost {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unmetered => "unmetered",
            Self::Metered => "metered",
            Self::Unknown => "unknown",
        }
    }
}

impl FromStr for HostNetworkCost {
    type Err = VolunteerInputError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "unmetered" => Ok(Self::Unmetered),
            "metered" => Ok(Self::Metered),
            "unknown" => Ok(Self::Unknown),
            _ => Err(VolunteerInputError::NetworkCost(value.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerSource {
    Ac,
    Battery,
    Unknown,
}

impl PowerSource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ac => "ac",
            Self::Battery => "battery",
            Self::Unknown => "unknown",
        }
    }
}

impl FromStr for PowerSource {
    type Err = VolunteerInputError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "ac" => Ok(Self::Ac),
            "battery" => Ok(Self::Battery),
            "unknown" => Ok(Self::Unknown),
            _ => Err(VolunteerInputError::PowerSource(value.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolunteerInputs {
    pub consent: VolunteerConsent,
    pub network_cost: HostNetworkCost,
    pub power_source: PowerSource,
}

impl Default for VolunteerInputs {
    fn default() -> Self {
        Self {
            consent: VolunteerConsent::Undecided,
            network_cost: HostNetworkCost::Unknown,
            power_source: PowerSource::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolunteerBlockReason {
    Eligible,
    ConsentRequired,
    Declined,
    MeteredNetwork,
    BatteryPower,
    HostConditionsUnknown,
}

impl VolunteerBlockReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Eligible => "eligible",
            Self::ConsentRequired => "consent_required",
            Self::Declined => "declined",
            Self::MeteredNetwork => "metered_network",
            Self::BatteryPower => "battery_power",
            Self::HostConditionsUnknown => "host_conditions_unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VolunteerDecision {
    pub enable_discovery_server: bool,
    pub enable_relay_server: bool,
    pub enable_upnp: bool,
    pub reason: VolunteerBlockReason,
}

pub struct VolunteerPolicy;

impl VolunteerPolicy {
    pub const fn evaluate(inputs: VolunteerInputs) -> VolunteerDecision {
        match inputs.consent {
            VolunteerConsent::Undecided => {
                return VolunteerDecision::disabled(VolunteerBlockReason::ConsentRequired)
            }
            VolunteerConsent::Declined => {
                return VolunteerDecision::disabled(VolunteerBlockReason::Declined)
            }
            VolunteerConsent::Granted => {}
        }
        if matches!(inputs.network_cost, HostNetworkCost::Metered) {
            return VolunteerDecision::disabled(VolunteerBlockReason::MeteredNetwork);
        }
        if matches!(inputs.power_source, PowerSource::Battery) {
            return VolunteerDecision::disabled(VolunteerBlockReason::BatteryPower);
        }
        if matches!(inputs.network_cost, HostNetworkCost::Unknown)
            || matches!(inputs.power_source, PowerSource::Unknown)
        {
            return VolunteerDecision {
                enable_discovery_server: true,
                enable_relay_server: false,
                enable_upnp: true,
                reason: VolunteerBlockReason::HostConditionsUnknown,
            };
        }
        VolunteerDecision {
            enable_discovery_server: true,
            enable_relay_server: true,
            enable_upnp: true,
            reason: VolunteerBlockReason::Eligible,
        }
    }
}

impl VolunteerDecision {
    const fn disabled(reason: VolunteerBlockReason) -> Self {
        Self {
            enable_discovery_server: false,
            enable_relay_server: false,
            enable_upnp: false,
            reason,
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum VolunteerInputError {
    #[error("志愿授权状态无效：{0}")]
    Consent(String),
    #[error("网络成本状态无效：{0}")]
    NetworkCost(String),
    #[error("供电状态无效：{0}")]
    PowerSource(String),
}

impl fmt::Display for VolunteerDecision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "discovery={}, relay={}, upnp={}, reason={}",
            self.enable_discovery_server,
            self.enable_relay_server,
            self.enable_upnp,
            self.reason.as_str()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 未授权或资源不合格时完全禁用志愿服务() {
        for (inputs, reason) in [
            (
                VolunteerInputs::default(),
                VolunteerBlockReason::ConsentRequired,
            ),
            (
                VolunteerInputs {
                    consent: VolunteerConsent::Declined,
                    network_cost: HostNetworkCost::Unmetered,
                    power_source: PowerSource::Ac,
                },
                VolunteerBlockReason::Declined,
            ),
            (
                VolunteerInputs {
                    consent: VolunteerConsent::Granted,
                    network_cost: HostNetworkCost::Metered,
                    power_source: PowerSource::Ac,
                },
                VolunteerBlockReason::MeteredNetwork,
            ),
            (
                VolunteerInputs {
                    consent: VolunteerConsent::Granted,
                    network_cost: HostNetworkCost::Unmetered,
                    power_source: PowerSource::Battery,
                },
                VolunteerBlockReason::BatteryPower,
            ),
        ] {
            let decision = VolunteerPolicy::evaluate(inputs);
            assert!(!decision.enable_discovery_server);
            assert!(!decision.enable_relay_server);
            assert!(!decision.enable_upnp);
            assert_eq!(decision.reason, reason);
        }
    }

    #[test]
    fn 未知宿主条件只启用低成本发现候选() {
        let decision = VolunteerPolicy::evaluate(VolunteerInputs {
            consent: VolunteerConsent::Granted,
            network_cost: HostNetworkCost::Unknown,
            power_source: PowerSource::Ac,
        });
        assert!(decision.enable_discovery_server);
        assert!(!decision.enable_relay_server);
        assert!(decision.enable_upnp);
        assert_eq!(decision.reason, VolunteerBlockReason::HostConditionsUnknown);
    }

    #[test]
    fn 已授权交流电非计费设备启用完整志愿能力() {
        let decision = VolunteerPolicy::evaluate(VolunteerInputs {
            consent: VolunteerConsent::Granted,
            network_cost: HostNetworkCost::Unmetered,
            power_source: PowerSource::Ac,
        });
        assert!(decision.enable_discovery_server);
        assert!(decision.enable_relay_server);
        assert!(decision.enable_upnp);
        assert_eq!(decision.reason, VolunteerBlockReason::Eligible);
    }
}
