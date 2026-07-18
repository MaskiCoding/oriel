//! `WindowServer` integration: enumeration, batched state queries, event tap, Space topology.

use skylight_sys::SkyLight;

pub struct Capabilities {
    pub connection: Option<i32>,
    pub missing: Vec<&'static str>,
}

pub fn probe() -> Capabilities {
    let Some(sl) = SkyLight::load() else {
        return Capabilities {
            connection: None,
            missing: vec!["SkyLight framework"],
        };
    };
    Capabilities {
        connection: sl.SLSMainConnectionID.map(|f| unsafe { f() }),
        missing: sl.missing(),
    }
}
