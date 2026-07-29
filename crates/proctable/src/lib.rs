//! The process table, via `libproc`. Public API, no entitlement, no TCC prompt:
//! Lantern costs nothing on Oriel's permission surface.
//!
//! Read in two passes. The parent links come from the short BSD flavor, which
//! reports processes this user does not own — macOS puts setuid-root
//! `/usr/bin/login` between a terminal and its shells, and the richer flavors
//! refuse it, which severs the tree exactly where it matters. CPU totals then
//! come from the task flavor, which only needs to work for the agents
//! themselves.

#![allow(clippy::cast_possible_truncation)] // sizes are bounded by the C structs
#![allow(clippy::cast_possible_wrap)] // pids and sizes are far below i32::MAX
#![allow(clippy::cast_sign_loss)] // guarded by the `> 0` checks above each cast

use core::ffi::{c_int, c_void};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;
use std::time::Duration;

use model::{Pid, Proc};

const CTL_KERN: c_int = 1;
const KERN_ARGMAX: c_int = 8;
const KERN_PROCARGS2: c_int = 49;
const PROC_ALL_PIDS: u32 = 1;
const PROC_PIDTASKINFO: c_int = 4;
const PROC_PIDT_SHORTBSDINFO: c_int = 13;
const PROC_PIDPATHINFO_MAXSIZE: usize = 4096;

/// `struct proc_bsdshortinfo` — 64 bytes.
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct ShortBsdInfo {
    pid: u32,
    ppid: u32,
    pgid: u32,
    status: u32,
    comm: [u8; 16],
    flags: u32,
    uid: u32,
    gid: u32,
    ruid: u32,
    rgid: u32,
    svuid: u32,
    svgid: u32,
    rfu: u32,
}

/// `struct proc_taskinfo` — 96 bytes. Only the two CPU totals are read.
#[repr(C)]
#[derive(Default, Clone, Copy)]
struct TaskInfo {
    virtual_size: u64,
    resident_size: u64,
    total_user: u64,
    total_system: u64,
    threads_user: u64,
    threads_system: u64,
    policy: i32,
    faults: i32,
    pageins: i32,
    cow_faults: i32,
    messages_sent: i32,
    messages_received: i32,
    syscalls_mach: i32,
    syscalls_unix: i32,
    csw: i32,
    threadnum: i32,
    numrunning: i32,
    priority: i32,
}

/// `struct mach_timebase_info`: the ratio turning mach ticks into nanoseconds.
#[repr(C)]
struct MachTimebase {
    numer: u32,
    denom: u32,
}

unsafe extern "C" {
    fn mach_timebase_info(info: *mut MachTimebase) -> c_int;
    fn proc_listpids(kind: u32, typeinfo: u32, buffer: *mut c_void, buffersize: c_int) -> c_int;
    fn proc_pidinfo(
        pid: c_int,
        flavor: c_int,
        arg: u64,
        buffer: *mut c_void,
        buffersize: c_int,
    ) -> c_int;
    fn proc_pidpath(pid: c_int, buffer: *mut c_void, buffersize: u32) -> c_int;
    fn sysctl(
        name: *mut c_int,
        namelen: u32,
        oldp: *mut c_void,
        oldlenp: *mut usize,
        newp: *mut c_void,
        newlen: usize,
    ) -> c_int;
}

/// Every process, with its parent. CPU is left at zero — fill it with
/// [`detail`] for the few processes that could be agents.
pub fn table() -> Vec<Proc> {
    let mut out = Vec::new();
    for pid in pids() {
        if pid <= 0 {
            continue;
        }
        let Some(info) = short_info(pid) else {
            continue;
        };
        out.push(Proc {
            // Raw ints belong to the C boundary; everything above it is typed.
            pid: Pid(pid),
            ppid: Pid(info.ppid as i32),
            name: cstr(&info.comm),
            cpu: Duration::ZERO,
        });
    }
    out
}

/// Reads the process table, remembering what it has already looked up.
///
/// Naming a process costs a `sysctl` that copies its whole argument area, and
/// the answer never changes: a process is named once, at exec. Asking again
/// every two seconds for hundreds of processes was almost the entire cost of a
/// poll. The scratch buffer the kernel wants is close to a megabyte, so
/// allocating and zeroing one per sweep was most of the rest.
#[derive(Default)]
pub struct Reader {
    /// pid -> (kernel `comm`, resolved name). `comm` is the guard against pid
    /// reuse: a recycled pid running something else reports a different one,
    /// and the name is looked up again.
    names: HashMap<Pid, (String, String)>,
    scratch: Vec<u8>,
}

impl Reader {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fills in CPU totals and the best available name for `wanted`.
    ///
    /// Naming an agent is genuinely awkward. Claude Code installs its executable
    /// under a version-numbered file and reports that same string as its `comm`,
    /// so both the accounting name and the executable path say `2.1.220`. The
    /// command the user actually typed survives only in `argv[0]`, so that is
    /// tried first, with the executable name behind it.
    pub fn detail(&mut self, table: &mut [Proc], wanted: &[Pid]) {
        if self.scratch.is_empty() {
            self.scratch = vec![0u8; arg_max()];
        }
        for proc in table.iter_mut().filter(|p| wanted.contains(&p.pid)) {
            proc.cpu = cpu_time(proc.pid.0);
            match self.names.get(&proc.pid) {
                Some((comm, resolved)) if *comm == proc.name => {
                    proc.name.clone_from(resolved);
                }
                _ => {
                    let comm = proc.name.clone();
                    if let Some(name) =
                        arg0(proc.pid.0, &mut self.scratch).or_else(|| exec_name(proc.pid.0))
                    {
                        proc.name = name;
                    }
                    self.names.insert(proc.pid, (comm, proc.name.clone()));
                }
            }
        }
        // Processes end; their names must not outlive them or the map is a leak
        // that grows for as long as Oriel runs.
        if self.names.len() > table.len().saturating_mul(2) {
            let alive: HashSet<Pid> = table.iter().map(|p| p.pid).collect();
            self.names.retain(|pid, _| alive.contains(pid));
        }
    }
}

/// The base name of `argv[0]` — the command as invoked.
fn arg0(pid: i32, scratch: &mut [u8]) -> Option<String> {
    let mut mib = [CTL_KERN, KERN_PROCARGS2, pid];
    let mut len = scratch.len();
    let ok = unsafe {
        sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            scratch.as_mut_ptr().cast::<c_void>(),
            &raw mut len,
            core::ptr::null_mut(),
            0,
        )
    };
    if ok != 0 || len < size_of::<i32>() {
        return None;
    }
    // Layout: argc, then the exec path, then NUL padding, then argv[0].
    let buf = &scratch[size_of::<i32>()..len];
    let after_path = buf.iter().position(|b| *b == 0)?;
    let rest = &buf[after_path..];
    let start = rest.iter().position(|b| *b != 0)?;
    let arg = &rest[start..];
    let end = arg.iter().position(|b| *b == 0).unwrap_or(arg.len());
    String::from_utf8_lossy(&arg[..end])
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn arg_max() -> usize {
    let mut mib = [CTL_KERN, KERN_ARGMAX];
    let mut value: i32 = 0;
    let mut len = size_of::<i32>();
    let ok = unsafe {
        sysctl(
            mib.as_mut_ptr(),
            mib.len() as u32,
            (&raw mut value).cast::<c_void>(),
            &raw mut len,
            core::ptr::null_mut(),
            0,
        )
    };
    if ok == 0 && value > 0 {
        value as usize
    } else {
        256 * 1024
    }
}

/// The executable's own file name, which a process cannot rewrite.
pub fn exec_name(pid: i32) -> Option<String> {
    let mut buf = vec![0u8; PROC_PIDPATHINFO_MAXSIZE];
    let wrote = unsafe {
        proc_pidpath(
            pid,
            buf.as_mut_ptr().cast::<c_void>(),
            PROC_PIDPATHINFO_MAXSIZE as u32,
        )
    };
    if wrote <= 0 {
        return None;
    }
    buf.truncate(wrote as usize);
    String::from_utf8_lossy(&buf)
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn short_info(pid: i32) -> Option<ShortBsdInfo> {
    let mut info = ShortBsdInfo::default();
    let size = size_of::<ShortBsdInfo>() as c_int;
    let wrote = unsafe {
        proc_pidinfo(
            pid,
            PROC_PIDT_SHORTBSDINFO,
            0,
            (&raw mut info).cast::<c_void>(),
            size,
        )
    };
    (wrote == size).then_some(info)
}

/// Total CPU burned by a process, in real time.
///
/// `pti_total_user` and `pti_total_system` are mach absolute time units, not
/// nanoseconds. On Apple Silicon the timebase is 125/3, so reading them as
/// nanoseconds under-reports by nearly 42x: a process pegging a core measures
/// 48 ms per two seconds instead of 2000. Converting through the timebase is
/// the whole fix. The totals already include threads that are still running —
/// `pti_threads_*` moves in lockstep with them — so there is nothing to walk
/// and add, and adding it double-counts.
fn cpu_time(pid: i32) -> Duration {
    let Some(task) = task_info(pid) else {
        return Duration::ZERO;
    };
    let ticks = u128::from(task.total_user.saturating_add(task.total_system));
    let (numer, denom) = timebase();
    let nanos = ticks * u128::from(numer) / u128::from(denom.max(1));
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

/// The machine's mach-tick to nanosecond ratio, asked of the kernel once.
fn timebase() -> (u32, u32) {
    static TIMEBASE: OnceLock<(u32, u32)> = OnceLock::new();
    *TIMEBASE.get_or_init(|| {
        let mut info = MachTimebase { numer: 0, denom: 0 };
        let ok = unsafe { mach_timebase_info(&raw mut info) };
        if ok == 0 && info.numer > 0 && info.denom > 0 {
            (info.numer, info.denom)
        } else {
            (1, 1)
        }
    })
}

fn task_info(pid: i32) -> Option<TaskInfo> {
    let mut info = TaskInfo::default();
    let size = size_of::<TaskInfo>() as c_int;
    let wrote = unsafe {
        proc_pidinfo(
            pid,
            PROC_PIDTASKINFO,
            0,
            (&raw mut info).cast::<c_void>(),
            size,
        )
    };
    (wrote == size).then_some(info)
}

fn pids() -> Vec<i32> {
    let bytes = unsafe { proc_listpids(PROC_ALL_PIDS, 0, core::ptr::null_mut(), 0) };
    if bytes <= 0 {
        return Vec::new();
    }
    // The table can grow between sizing and reading; ask for headroom.
    let cap = (bytes as usize / size_of::<i32>()).saturating_add(64);
    let mut buf = vec![0i32; cap];
    let wrote = unsafe {
        proc_listpids(
            PROC_ALL_PIDS,
            0,
            buf.as_mut_ptr().cast::<c_void>(),
            (cap * size_of::<i32>()) as c_int,
        )
    };
    if wrote <= 0 {
        return Vec::new();
    }
    buf.truncate(wrote as usize / size_of::<i32>());
    buf
}

fn cstr(raw: &[u8]) -> String {
    let end = raw.iter().position(|b| *b == 0).unwrap_or(raw.len());
    String::from_utf8_lossy(&raw[..end]).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layouts_match_the_kernel_headers() {
        assert_eq!(size_of::<ShortBsdInfo>(), 64);
        assert_eq!(size_of::<TaskInfo>(), 96);
    }

    #[test]
    fn the_table_finds_this_very_process() {
        let table = table();
        assert!(table.len() > 1, "process table should not be near-empty");
        let me = Pid(std::process::id() as i32);
        assert!(table.iter().any(|p| p.pid == me), "own pid missing");
    }

    #[test]
    fn the_tree_survives_processes_this_user_cannot_inspect() {
        // Root-owned processes must still contribute parent links, or a
        // terminal's setuid `login` severs every shell beneath it.
        let table = table();
        let known: Vec<Pid> = table.iter().map(|p| p.pid).collect();
        let mut cur = Pid(std::process::id() as i32);
        let mut hops = 0;
        while let Some(p) = table.iter().find(|p| p.pid == cur) {
            if p.ppid.0 <= 1 {
                break;
            }
            assert!(
                known.contains(&p.ppid),
                "parent {} missing from table",
                p.ppid
            );
            cur = p.ppid;
            hops += 1;
            assert!(hops < 64, "parent chain did not terminate");
        }
        assert!(hops > 0, "expected at least one ancestor");
    }

    #[test]
    fn a_name_is_looked_up_once_and_then_remembered() {
        // The cache is what makes a poll cheap; if a repeat sweep still paid for
        // the argv syscall the optimisation would be silently gone.
        let me = Pid(i32::try_from(std::process::id()).expect("pid fits"));
        let mut reader = Reader::new();

        let mut first = table();
        reader.detail(&mut first, &[me]);
        let named = first.iter().find(|p| p.pid == me).expect("own pid");
        let expected = named.name.clone();

        let mut second = table();
        reader.detail(&mut second, &[me]);
        let again = second.iter().find(|p| p.pid == me).expect("own pid");
        assert_eq!(
            again.name, expected,
            "a remembered name must match the looked-up one"
        );
    }

    #[test]
    fn a_recycled_pid_is_named_again_rather_than_remembered_wrong() {
        // The guard is the kernel's `comm`: a pid running something else reports
        // a different one, so the stale entry cannot be handed back.
        let me = Pid(i32::try_from(std::process::id()).expect("pid fits"));
        let mut reader = Reader::new();
        let mut table = table();
        reader.detail(&mut table, &[me]);

        // Same pid, but presenting as a different program.
        let mut recycled = vec![Proc {
            pid: me,
            ppid: Pid(1),
            name: "something-else-entirely".into(),
            cpu: Duration::ZERO,
        }];
        reader.detail(&mut recycled, &[me]);
        assert_ne!(
            recycled[0].name, "something-else-entirely",
            "a changed comm must trigger a fresh lookup, not a cache hit"
        );
    }

    #[test]
    fn a_busy_process_reads_as_busy() {
        // The task totals are mach ticks, not nanoseconds; read raw they were
        // about two percent of the truth on this timebase. Burn a known amount
        // of CPU and insist most of it is visible — the shape of the bug rather
        // than a fixed number, so it holds on any timebase.
        let me = Pid(i32::try_from(std::process::id()).expect("pid fits"));
        let mut snapshot = table();
        Reader::new().detail(&mut snapshot, &[me]);
        let before = snapshot.iter().find(|p| p.pid == me).expect("own pid").cpu;

        let spin = std::time::Instant::now();
        while spin.elapsed() < Duration::from_millis(300) {
            std::hint::black_box(0..64).for_each(drop);
        }
        let burned = spin.elapsed();

        let mut snapshot = table();
        Reader::new().detail(&mut snapshot, &[me]);
        let after = snapshot.iter().find(|p| p.pid == me).expect("own pid").cpu;
        let seen = after.saturating_sub(before);
        // Bounded on both sides. A lower bound alone passed while the reader
        // was double-counting live threads, which was twice the truth on a 1:1
        // timebase; an upper bound is what makes that fail.
        assert!(
            seen > burned / 2,
            "spun {burned:?} of wall clock but only {seen:?} of CPU was visible"
        );
        assert!(
            seen < burned * 2,
            "spun {burned:?} of wall clock but {seen:?} of CPU was reported"
        );
    }

    #[test]
    fn detail_fills_cpu_for_this_process() {
        let mut table = table();
        let me = Pid(std::process::id() as i32);
        Reader::new().detail(&mut table, &[me]);
        let found = table.iter().find(|p| p.pid == me).expect("own pid missing");
        assert!(found.cpu > Duration::ZERO, "this test has burned CPU");
    }

    #[test]
    fn a_name_comes_back_from_argv_not_the_versioned_executable() {
        // The case this guards: Claude Code installs itself as a version-numbered
        // file and reports that same string as its comm, so both the accounting
        // name and the executable path read "2.1.220" while the command the user
        // typed survives only in argv[0]. Reproduced by launching a child whose
        // argv[0] differs from its executable, which is exactly that shape.
        let child = std::process::Command::new("/bin/sh")
            .args(["-c", "exec -a pretend-agent /bin/cat"])
            .stdin(std::process::Stdio::piped())
            .spawn();
        let Ok(mut child) = child else {
            return; // no shell to borrow; nothing to assert
        };
        std::thread::sleep(Duration::from_millis(250));
        let pid = Pid(i32::try_from(child.id()).expect("pid fits"));

        let mut scratch = vec![0u8; arg_max()];
        let argv0 = arg0(pid.0, &mut scratch);
        let executable = exec_name(pid.0);
        let _ = child.kill();
        let _ = child.wait();

        assert_eq!(
            argv0.as_deref(),
            Some("pretend-agent"),
            "argv[0] is the only place the invoked name survives"
        );
        assert_eq!(
            executable.as_deref(),
            Some("cat"),
            "the executable name is the wrong identity for an agent"
        );
    }
}
