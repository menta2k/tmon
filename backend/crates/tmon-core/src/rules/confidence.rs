//! How sure a categorization is, judged from the evidence its rule matched on
//! (the flow-data analogue of nDPI's "detected" vs "guessed" protocols).

use serde::Serialize;

use super::model::RuleMatch;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    /// No rule matched.
    None,
    /// Only ports, protocol, direction or interface: a port can carry anything.
    Guessed,
    /// The remote network (ASN, CIDR, country) or a specific local device.
    Inferred,
    /// The remote host name (from DNS) or a confidently classified
    /// application identifies the service.
    Detected,
}

impl Confidence {
    pub fn as_str(&self) -> &'static str {
        match self {
            Confidence::None => "none",
            Confidence::Guessed => "guessed",
            Confidence::Inferred => "inferred",
            Confidence::Detected => "detected",
        }
    }
}

impl RuleMatch {
    /// Confidence of a categorization made by this match: its strongest
    /// evidence. Application rules report their best case ("detected"); per
    /// flow, a guessed application lowers it to "inferred" (see
    /// `CompiledRule::confidence_for`).
    pub fn confidence(&self) -> Confidence {
        if self.hostname.is_some() || self.app.is_some() || self.app_category.is_some() {
            Confidence::Detected
        } else if self.asn.is_some()
            || self.remote_cidr.is_some()
            || self.country.is_some()
            || self.local_host.is_some()
        {
            Confidence::Inferred
        } else {
            Confidence::Guessed
        }
    }
}
