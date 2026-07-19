use skylight_sys::Psn;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn GetProcessForPID(pid: i32, psn: *mut Psn) -> i32;
}

/// `_SLPSSetFrontProcessWithOptions` mode `kCPSUserGenerated`: attribute the
/// activation to the user so the `WindowServer` treats it as a real switch
/// (raises the window and changes Space for off-Space targets).
const USER_GENERATED: u32 = 0x200;

fn psn_for_pid(pid: i32) -> Option<Psn> {
    let mut psn = Psn::default();
    (unsafe { GetProcessForPID(pid, &raw mut psn) } == 0).then_some(psn)
}

/// A synthetic event record that nudges the `WindowServer` to make `wid` the key
/// window of its process. The offsets are the private event-record ABI, not a
/// layout we get to choose; `kind` is 0x01 then 0x02 for the down/up pair.
fn key_event_record(wid: u32, kind: u8) -> [u8; 0xf8] {
    let mut bytes = [0u8; 0xf8];
    bytes[0x04] = 0xf8;
    bytes[0x08] = kind;
    bytes[0x20..0x30].fill(0xff);
    bytes[0x3a] = 0x10;
    bytes[0x3c..0x40].copy_from_slice(&wid.to_ne_bytes());
    bytes
}

impl super::WindowServer {
    /// Fronts `pid` and raises window `wid`: activates the process (raising the
    /// window and switching Space for off-Space targets), then posts the
    /// synthetic key-event pair so the window actually becomes key.
    ///
    /// This is the `WindowServer` half of the focus sequence; AX un-minimize and
    /// `AXRaise` within the app's own stack are layered on separately.
    pub fn focus_window(&self, pid: i32, wid: u32) -> bool {
        let Some(psn) = psn_for_pid(pid) else {
            return false;
        };
        let set_front = self.sl._SLPSSetFrontProcessWithOptions.unwrap();
        let post = self.sl.SLPSPostEventRecordTo.unwrap();
        let psn = &raw const psn;
        let fronted = unsafe { set_front(psn, wid, USER_GENERATED) } == 0;
        for kind in [0x01, 0x02] {
            let record = key_event_record(wid, kind);
            unsafe { post(psn, record.as_ptr()) };
        }
        fronted
    }
}
